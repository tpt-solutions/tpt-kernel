// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k kernel main binary: early boot entry, paging, the syscall boundary,
//! address spaces, panic handler, and the Phase 2 demo.
//!
//! The boot loader jumps to `_start` with interrupts in an unknown state and
//! no stack of ours. `_start` disables interrupts, switches to a kernel stack,
//! and calls `kmain`.
//!
//! The demo runs these threads under the scheduler:
//!
//! - two workers that print tagged letters and spin between prints, so the PIT
//!   tick preempts them;
//! - a kernel server that receives one request and replies;
//! - a user thread: a ring-3 program that writes a message, checks that a
//!   receive-only handle is refused, sends a request and receives the reply
//!   through capability handles, then exits with the reply's first word;
//! - two user processes, each in its own address space. Each writes its number
//!   to the same virtual address, yields until the other has had time to write
//!   its number too, then reads its own back. Isolation means each reads its
//!   own number.
//!
//! Kernel state (the scheduler, the capability table and the endpoints) is
//! touched only inside `interrupts::without` sections, or by a syscall, which
//! runs with interrupts off. The kernel is single-core, so that is enough.

#![no_std]
#![no_main]

use core::arch::{asm, naked_asm};
use core::fmt::Write as _;
use core::panic::PanicInfo;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

use tpt_k_kernel_arch_x86_64::paging::{self, PageTables, Table, NO_CACHE, USER, WRITABLE};
use tpt_k_kernel_arch_x86_64::{
    apic, gdt, idt, interrupts, ioapic, pic, qemu, rtc, serial, syscall,
};
use tpt_k_kernel_ipc::cap::{delete_across, grant, revoke_across, CapError, CapTable};
use tpt_k_kernel_ipc::endpoint::{Endpoint, Message, RecvOutcome, SendOutcome, Transfer};
use tpt_k_kernel_ipc::{CapId, ObjectId, Rights};
use tpt_k_kernel_mem::bootmap;
use tpt_k_kernel_mem::{Frame as PhysFrame, FrameAllocator, FRAME_SIZE};
use tpt_k_kernel_sched::{Scheduler, State, MAX_THREADS};

/// Words per kernel thread stack (4 KiB).
/// APIC timer counts per 10 ms under the `replay` feature. A calibrated count
/// moves with host timing, which moves the preemption points and so the order
/// the demo threads finish in. A fixed count keeps the trace the same on every
/// run under the instruction counter. Normal builds calibrate.
#[cfg(feature = "replay")]
const REPLAY_TIMER_COUNTS: u32 = 625_000;

const THREAD_STACK_WORDS: usize = 512;

/// A thread stack. The alignment keeps its end 16-byte aligned. A thread's
/// stack is also its kernel stack: syscalls and ring-3 interrupts run on it.
#[repr(C, align(16))]
struct ThreadStack([u64; THREAD_STACK_WORDS]);

/// A larger kernel stack, for threads that make syscalls.
#[repr(C, align(16))]
struct UserThreadStack([u64; 2048]);

static mut STACK_A: ThreadStack = ThreadStack([0; THREAD_STACK_WORDS]);
static mut STACK_B: ThreadStack = ThreadStack([0; THREAD_STACK_WORDS]);
static mut STACK_SERVER: ThreadStack = ThreadStack([0; THREAD_STACK_WORDS]);
static mut STACK_USER: UserThreadStack = UserThreadStack([0; 2048]);
static mut STACK_PROCESS0: UserThreadStack = UserThreadStack([0; 2048]);
static mut STACK_PROCESS1: UserThreadStack = UserThreadStack([0; 2048]);
static mut STACK_SHM_SENDER: UserThreadStack = UserThreadStack([0; 2048]);
static mut STACK_SHM_RECEIVER: UserThreadStack = UserThreadStack([0; 2048]);

/// Entries of the zero-copy demo threads: the sender, then the receiver.
static mut SHM_ENTRY: [Option<UserEntry>; 2] = [None; 2];

/// The thread table. Only the running thread touches it.
static mut SCHED: Scheduler = Scheduler::new();

/// Per-thread context, indexed by scheduler slot. The kernel stack is used for
/// syscalls and ring-3 interrupts. The root is the address space, which is
/// loaded into CR3 when the thread is switched in.
#[derive(Clone, Copy)]
struct Context {
    kstack_top: u64,
    cr3: u64,
}

static mut CONTEXTS: [Context; MAX_THREADS] = [Context {
    kstack_top: 0,
    cr3: 0,
}; MAX_THREADS];

/// Capability tables, one per scheduler slot. A handle is valid only in the table
/// of the thread it was given to, so a thread cannot use another thread's handles.
static mut CAP_TABLES: [CapTable<8>; MAX_THREADS] = [CapTable::new(); MAX_THREADS];

/// The demo endpoints, indexed by object id.
static mut ENDPOINTS: [Endpoint<4>; 4] = [Endpoint::new(); 4];

/// Handles of the server thread: the request receive handle and the reply send
/// handle. Set by the bootstrap thread before the server runs.
static mut SERVER_HANDLES: Option<[CapId; 2]> = None;

/// Handles of the user IPC thread: the request send handle, the reply receive
/// handle, and a receive-only request handle for the refusal check.
static mut USER_HANDLES: Option<[CapId; 3]> = None;

/// Frames the kernel tracks: the first 512 MiB of physical memory.
const COVERED_FRAMES: usize = 1 << 17;

/// Allocation bitmap for those frames: one bit per frame.
static mut FRAME_BITS: [u64; COVERED_FRAMES / 64] = [0; COVERED_FRAMES / 64];

/// Page tables for the kernel's address spaces. The pool lives in `.bss`, under
/// the identity map that the firmware provides until `setup_paging`.
static mut PT_POOL: [Table; 128] = [Table::EMPTY; 128];

/// Allocation bitmap for the page-table pool: one bit per table.
static mut PT_BITS: [u64; 2] = [0; 2];

/// The kernel's page tables, kept after setup so that syscalls can map pages
/// into the address spaces of user threads.
static mut KERNEL_TABLES: Option<PageTables<'static>> = None;

/// The boot address space's CR3 value.
static mut BOOT_CR3: u64 = 0;

/// Stacks of the threads that user threads spawn, their address spaces, and
/// whether each slot is in use. A spawned thread's entry is in `SPAWN_ENTRY`.
#[repr(C, align(16))]
struct SpawnStack([u64; 2048]);

static mut SPAWN_STACKS: [SpawnStack; 2] = [SpawnStack([0; 2048]), SpawnStack([0; 2048])];
static mut SPAWN_USED: [bool; 2] = [false; 2];
static mut SPAWN_ENTRY: [Option<UserEntry>; 2] = [None; 2];
static mut CHILD_ROOTS: [(usize, u64); 2] = [(0, 0); 2];

/// Where a spawned program image is mapped in its child's address space.
const IMAGE_BASE: u64 = 0x2000_0000;
/// Largest program image `SYS_SPAWN_IMAGE` accepts: one page.
const IMAGE_MAX: u64 = 4096;

/// One page that holds a spawned program's image. Each spawn slot has its own
/// page, and the kernel reaches it through the identity mapping.
#[repr(C, align(4096))]
struct ImagePage([u8; 4096]);

static mut IMAGE_PAGES: [ImagePage; 2] = [ImagePage([0; 4096]), ImagePage([0; 4096])];

/// The root thread's stack and entry.
static mut STACK_ROOT: UserThreadStack = UserThreadStack([0; 2048]);
static mut ROOT_ENTRY: Option<UserEntry> = None;

/// The driver thread's kernel stack, and its entry (set by the bootstrap).
static mut STACK_DRIVER: UserThreadStack = UserThreadStack([0; 2048]);
static mut STACK_FAULT: UserThreadStack = UserThreadStack([0; 2048]);
static mut DRIVER_ENTRY: Option<UserEntry> = None;

/// The driver: a user thread that waits for the RTC's interrupt through its
/// interrupt object, then exits with the number of interrupts it saw.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn driver_program() -> ! {
    naked_asm!(
        // rdi holds the interrupt capability on entry.
        "mov eax, {irq_wait}",
        "syscall",
        "mov rdi, rax",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        irq_wait = const SYS_IRQ_WAIT,
        exit = const SYS_EXIT,
    )
}

/// Kernel entry of the driver thread: enters ring 3 with `DRIVER_ENTRY`.
extern "C" fn driver_thread_entry() -> ! {
    // SAFETY: set by the bootstrap thread before the thread first runs.
    let spec =
        unsafe { *addr_of_mut!(DRIVER_ENTRY) }.unwrap_or_else(|| fail("driver entry missing"));
    enter_spec(spec)
}

/// The shared page of the zero-copy demo, mapped by both user threads.
static mut SHARED_FRAME: Frame = Frame([0; 512]);

/// A page-aligned frame.
#[repr(C, align(4096))]
struct Frame([u64; 512]);

static mut REMAP_FRAME: Frame = Frame([0; 512]);

/// Data frames of the two user processes. Each process maps its own frame at
/// `PROCESS_DATA`.
static mut PROCESS_FRAMES: [Frame; 2] = [Frame([0; 512]), Frame([0; 512])];

/// Address space roots of the two user processes, set by `setup_paging`.
static mut ROOTS: [(usize, u64); 4] = [(0, 0); 4];

// Bounds of the user pages, set by the linker script.
unsafe extern "C" {
    static __user_code_start: u8;
    static __user_code_end: u8;
    static __user_stack_start: u8;
    static __user_stack_end: u8;
}

/// Stack for the user programs, one page in the user-stack section.
#[repr(C, align(4096))]
struct UserStack([u8; 4096]);

#[unsafe(link_section = ".user_stack")]
static mut USER_STACK: UserStack = UserStack([0; 4096]);

/// Syscall numbers of the user ABI.
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 2;
const SYS_SEND: u64 = 3;
const SYS_RECV: u64 = 4;
const SYS_YIELD: u64 = 5;
const SYS_GRANT: u64 = 6;
const SYS_REVOKE: u64 = 7;
const SYS_MAP: u64 = 8;
const SYS_SPAWN: u64 = 9;
/// Spawns a child from a program image in the caller's memory. See `spawn_image`.
const SYS_SPAWN_IMAGE: u64 = 10;
/// Creates a memory object of `a0` pages and returns its capability. See
/// `create_memory`.
const SYS_MEM_CREATE: u64 = 11;
/// Sends a message with a capability: `a0` is the channel's send handle, `a1` the
/// message (four words), and `a2` the sender's handle to move, which needs GRANT.
const SYS_SEND_CAP: u64 = 12;
/// Receives a message and any capability with it: `a0` is the receive handle,
/// and `a1` a buffer of `RECV_CAP_BYTES` (the message, then the new handle).
const SYS_RECV_CAP: u64 = 13;
/// Sends a message with a bulk payload: `a0` is the channel's send handle, `a1`
/// the message (four words), and `a2` a descriptor of two words in user memory,
/// the payload's address and its length. See `send_bulk`.
const SYS_SEND_BULK: u64 = 14;
/// Waits for an interrupt on the interrupt object in `a0`. Returns how many have
/// arrived since the last wait. See `irq_wait`.
const SYS_IRQ_WAIT: u64 = 15;
/// Frees a memory object and returns its frames. See [`free_memory`].
const SYS_MEM_FREE: u64 = 16;

/// Virtual address where the shared page is mapped. Both demo threads use it.
/// The memory window: the only addresses a memory object may be mapped at. It
/// sits above the 4 GiB identity map, so a mapping here never shadows a physical
/// address the kernel reaches through the identity map.
const MEM_WINDOW_START: u64 = 0x1_0000_0000;
const MEM_WINDOW_END: u64 = 0x1_4000_0000;
/// Where the shared demo page is mapped: the start of the window.
const SHARED_VIRT: u64 = MEM_WINDOW_START;

/// Memory objects: capability object ids from `MEM_OBJECT_BASE` on, one per
/// slot of `MEM_OBJECTS`. Slot 0 is the shared demo page.
const MEM_OBJECT_BASE: u32 = 0x100;
const MAX_MEM_OBJECTS: usize = 8;
const MAX_OBJECT_PAGES: usize = 4;

/// A memory object: `count` physical pages, mapped in order by `SYS_MAP`.
#[derive(Clone, Copy)]
struct MemObject {
    pages: [u64; MAX_OBJECT_PAGES],
    count: usize,
    /// Set once `SYS_MAP` has mapped the object into an address space. There is
    /// no unmap yet, so a mapped object cannot be freed.
    mapped: bool,
}

static mut MEM_OBJECTS: [Option<MemObject>; MAX_MEM_OBJECTS] = [None; MAX_MEM_OBJECTS];

/// The frame allocator for run-time frames (memory objects), built from the
/// loader's memory map by `check_memory`.
static mut FRAME_ALLOC: Option<FrameAllocator<'static>> = None;
/// Value the sender writes to the shared page.
const SHM_MARKER: u64 = 0x00C0_FFEE;
/// Object ids: endpoints 0 to 2, then the memory object.
const CHANNEL_OBJECT: ObjectId = ObjectId(2);
/// The root and child channel. Its own endpoint: sharing `CHANNEL_OBJECT` let the
/// shared-page pair and the root thread take each other's messages.
const ROOT_CHANNEL_OBJECT: ObjectId = ObjectId(3);
const MEMORY_OBJECT: ObjectId = ObjectId(MEM_OBJECT_BASE);

/// Longest buffer a `write` call may name.
const MAX_WRITE: u64 = 256;

/// Size of the message buffer the user program uses (four words).
const MESSAGE_BYTES: u64 = 32;
/// Bytes a `SYS_RECV_CAP` buffer takes: the message, then the received handle.
const RECV_CAP_BYTES: u64 = MESSAGE_BYTES + 8;

/// Length of the message the user program writes.
const USER_MESSAGE_LEN: u64 = 18;

/// Virtual address where each user process keeps its number.
const PROCESS_DATA: u64 = 0x0040_0000;

/// Yields a process makes while the other process runs.
const PROCESS_YIELDS: u64 = 20;

/// Busy-wait iterations between a worker's prints. Long enough that ticks land
/// inside the spin.
const SPIN: u32 = 1_000_000;

/// Demo threads that must finish before the run ends: two workers, the server,
/// the user thread, two processes, and the driver.
const DEMO_THREADS: usize = 12;

/// Demo threads that have finished.
static FINISHED: AtomicUsize = AtomicUsize::new(0);

/// Timer ticks seen.
static TICKS: AtomicUsize = AtomicUsize::new(0);

/// Interrupts from the RTC, which the IO-APIC routes to `RTC_VECTOR`.
static RTC_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

/// The vector the RTC's IO-APIC route delivers to: the hardware line 8 vector.
const RTC_GSI: u32 = 8;
const RTC_VECTOR: u8 = idt::IRQ_BASE + 8;

/// Handles the RTC interrupt: clears the clock's flag, acknowledges the local
/// APIC, and counts the interrupt.
fn rtc_tick() {
    rtc::acknowledge();
    apic::eoi();
    RTC_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    // Record the interrupt for the interrupt object, and wake its waiter if one
    // is blocked. The scheduler is never mid-operation here: its operations run
    // with interrupts off.
    IRQ_PENDING.fetch_add(1, Ordering::Relaxed);
    let waiter = IRQ_WAITER.swap(0, Ordering::Relaxed);
    if waiter != 0 {
        // SAFETY: interrupts are off inside an interrupt handler, so nothing else
        // uses the scheduler.
        unsafe { (*addr_of_mut!(SCHED)).wake(waiter - 1) };
    }
}

/// Interrupts delivered to the interrupt object that the driver holds, and the
/// thread waiting for them (its slot plus one; zero if none).
static IRQ_PENDING: AtomicUsize = AtomicUsize::new(0);
static IRQ_WAITER: AtomicUsize = AtomicUsize::new(0);

/// The interrupt object for the RTC line. Its capability lets the holder wait
/// with `SYS_IRQ_WAIT`.
const IRQ_OBJECT: ObjectId = ObjectId(0x200);

/// Waits for an interrupt on the interrupt object named by `cap`, and returns how
/// many have arrived since the last wait. Blocks if none has.
fn irq_wait(cap: CapId) -> Result<u64, SysError> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // capability table and the scheduler.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let object = (*addr_of_mut!(CAP_TABLES))[me].lookup(cap, Rights::READ)?;
            if object != IRQ_OBJECT {
                return Err(SysError::Invalid);
            }
            let pending = IRQ_PENDING.swap(0, Ordering::Relaxed);
            if pending > 0 {
                return Ok(pending as u64);
            }
            // No handler can run between this store and the block, because
            // interrupts are off, so the wake cannot be lost.
            IRQ_WAITER.store(me + 1, Ordering::Relaxed);
            if !(*addr_of_mut!(SCHED)).block_current() {
                IRQ_WAITER.store(0, Ordering::Relaxed);
                return Err(SysError::Deadlock);
            }
            let mut out = serial::Writer;
            let _ = writeln!(out, "irq: driver woke");
            match IRQ_PENDING.swap(0, Ordering::Relaxed) {
                0 => Err(SysError::Invalid),
                count => Ok(count as u64),
            }
        }
    })
}

/// Routes the RTC through the IO-APIC and checks that it interrupts the
/// bootstrap CPU. Waits for two interrupts, or for three seconds of APIC timer
/// ticks, whichever comes first. Leaves the clock stopped and interrupts off.
fn check_ioapic_rtc() -> Result<(), &'static str> {
    if ioapic::version() == 0 {
        return Err("no IO-APIC");
    }
    ioapic::route(RTC_GSI, RTC_VECTOR, apic::id());
    idt::set_irq_handler(RTC_GSI as usize, rtc_tick);
    rtc::start_periodic(rtc::RATE_2_HZ);
    let start = TICKS.load(Ordering::Relaxed);
    interrupts::enable();
    while RTC_INTERRUPTS.load(Ordering::Relaxed) < 2
        && TICKS.load(Ordering::Relaxed).wrapping_sub(start) < 300
    {
        core::hint::spin_loop();
    }
    interrupts::disable();
    rtc::stop();
    let count = RTC_INTERRUPTS.load(Ordering::Relaxed);
    let mut out = serial::Writer;
    let _ = writeln!(out, "ioapic: rtc irqs={count}");
    if count < 2 {
        return Err("the RTC did not interrupt through the IO-APIC");
    }
    Ok(())
}

/// Ticks that gave the processor to another thread.
static PREEMPTIONS: AtomicUsize = AtomicUsize::new(0);

/// What a user thread enters with.
#[derive(Clone, Copy)]
struct UserEntry {
    entry: u64,
    stack_top: u64,
    a0: u64,
    a1: u64,
    a2: u64,
}

static mut USER_ENTRY: Option<UserEntry> = None;

/// What each user process enters with. The process number is passed in.
static mut PROCESS_ENTRY: [Option<UserEntry>; 2] = [None; 2];

/// The user-visible address range of a buffer, checked against `start..end`.
fn within(start: u64, end: u64, address: u64, len: u64) -> bool {
    address >= start && address.checked_add(len).is_some_and(|e| e <= end)
}

/// The user stack bounds, from the linker script.
fn user_stack_bounds() -> (u64, u64) {
    (
        core::ptr::addr_of!(__user_stack_start) as u64,
        core::ptr::addr_of!(__user_stack_end) as u64,
    )
}

/// Reads four message words from a user buffer. The buffer must lie in the user
/// stack page.
fn read_message(address: u64) -> Option<Message> {
    let (start, end) = user_stack_bounds();
    if !within(start, end, address, MESSAGE_BYTES) {
        return None;
    }
    let mut message = [0u64; 4];
    for (index, word) in message.iter_mut().enumerate() {
        // SAFETY: the buffer was checked to lie inside the user stack page, which
        // is mapped and readable (see `setup_paging`).
        *word = unsafe { ((address + index as u64 * 8) as *const u64).read_volatile() };
    }
    Some(message)
}

/// Writes four message words to a user buffer in the user stack page.
fn write_message(address: u64, message: Message) -> bool {
    let (start, end) = user_stack_bounds();
    if !within(start, end, address, MESSAGE_BYTES) {
        return false;
    }
    for (index, word) in message.iter().enumerate() {
        // SAFETY: as for `read_message`; the page is also writable.
        unsafe { ((address + index as u64 * 8) as *mut u64).write_volatile(*word) };
    }
    true
}

/// Writes a received message and its transfer to a user buffer: the four words,
/// then the new handle, or `u64::MAX` if no capability came with it. The buffer
/// is `RECV_CAP_BYTES` long and lies in the user stack page.
fn write_received(address: u64, message: Message, transfer: Transfer) -> bool {
    let (start, end) = user_stack_bounds();
    if !within(start, end, address, RECV_CAP_BYTES) {
        return false;
    }
    if !write_message(address, message) {
        return false;
    }
    let handle = transfer.map_or(u64::MAX, |cap| cap.raw());
    // SAFETY: the range was checked above; the page is writable.
    unsafe { ((address + MESSAGE_BYTES) as *mut u64).write_volatile(handle) };
    true
}

/// Why a system call failed. A failed call returns the negated errno value, so
/// every failure is in `-4095..=-1` as a `u64` (see [`is_error`]). The numbers
/// are those of `tpt-k-userland-libc::errno`. The kernel cannot depend on that
/// crate, so they are repeated here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SysError {
    /// A user pointer is outside the memory the call may use.
    Fault,
    /// An argument, or the kind of an object, is not one the call takes.
    Invalid,
    /// The handle is stale or was never valid.
    BadHandle,
    /// The capability lacks a right the call needs.
    Denied,
    /// The frame allocator has no frame left.
    NoMemory,
    /// A table, an object pool or an endpoint has no room left.
    Full,
    /// The call would block, and no other thread can run to wake it.
    Deadlock,
    /// The object is in use in a way the call cannot undo.
    Busy,
    /// The call number is not known.
    NoSys,
}

impl SysError {
    /// The errno value, as a positive number.
    const fn errno(self) -> u64 {
        match self {
            Self::Fault => 14,
            Self::Invalid => 22,
            Self::BadHandle => 9,
            Self::Denied => 13,
            Self::NoMemory => 12,
            Self::Full => 28,
            Self::Deadlock => 35,
            Self::Busy => 16,
            Self::NoSys => 38,
        }
    }

    /// The value a failed call returns in `rax`: the negated errno.
    const fn code(self) -> u64 {
        self.errno().wrapping_neg()
    }

    /// A short description, for the boot messages.
    fn as_str(self) -> &'static str {
        match self {
            Self::Fault => "bad user address",
            Self::Invalid => "invalid argument",
            Self::BadHandle => "bad handle",
            Self::Denied => "access denied",
            Self::NoMemory => "out of memory",
            Self::Full => "no room left",
            Self::Deadlock => "would deadlock",
            Self::Busy => "object is in use",
            Self::NoSys => "no such system call",
        }
    }
}

impl From<CapError> for SysError {
    fn from(err: CapError) -> Self {
        match err {
            CapError::InvalidHandle => Self::BadHandle,
            CapError::Denied | CapError::NotGrantable => Self::Denied,
            CapError::Full => Self::Full,
        }
    }
}

/// Whether a system call result is a failure code: the negated errno values,
/// `-4095..=-1`. Handles, slot numbers and byte counts are never that large.
fn is_error(raw: u64) -> bool {
    raw > (-4096i64) as u64
}

/// The syscall handler. `number` selects the call, and `a0`, `a1`, `a2` are its
/// arguments. A failed call returns a negated errno (see [`SysError`]).
extern "C" fn syscall_dispatch(number: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    match syscall(number, a0, a1, a2) {
        Ok(value) => value,
        Err(error) => error.code(),
    }
}

/// The body of [`syscall_dispatch`], with the failure as a [`SysError`].
fn syscall(number: u64, a0: u64, a1: u64, a2: u64) -> Result<u64, SysError> {
    Ok(match number {
        SYS_WRITE => {
            // The message lives in the user code pages, and the copy must stay
            // inside them.
            let start = core::ptr::addr_of!(__user_code_start) as u64;
            let end = core::ptr::addr_of!(__user_code_end) as u64;
            if a1 > MAX_WRITE {
                return Err(SysError::Invalid);
            }
            if !within(start, end, a0, a1) {
                return Err(SysError::Fault);
            }
            for offset in 0..a1 {
                // SAFETY: the range was checked to lie inside the user code pages,
                // which are mapped and readable.
                let byte = unsafe { ((a0 + offset) as *const u8).read_volatile() };
                serial::write_byte(byte);
            }
            a1
        }
        SYS_EXIT => {
            let mut out = serial::Writer;
            let _ = writeln!(out, "user: exit {a0}");
            release_handles();
            finish()
        }
        SYS_SEND => {
            let message = read_message(a1).ok_or(SysError::Fault)?;
            ipc_send(CapId::from_raw(a0), message)?;
            0
        }
        SYS_RECV => {
            let message = ipc_recv(CapId::from_raw(a0))?;
            if !write_message(a1, message) {
                return Err(SysError::Fault);
            }
            0
        }
        SYS_YIELD => {
            yield_now();
            0
        }
        SYS_GRANT => grant_handle(CapId::from_raw(a0), a1, a2)?,
        SYS_REVOKE => {
            revoke_handle(CapId::from_raw(a0))?;
            0
        }
        SYS_SPAWN => spawn_child(a0, CapId::from_raw(a1), a2)? as u64,
        SYS_SPAWN_IMAGE => spawn_image(a0)? as u64,
        SYS_MEM_CREATE => create_memory(a0)?.raw(),
        SYS_SEND_CAP => {
            let message = read_message(a1).ok_or(SysError::Fault)?;
            ipc_send_transfer(CapId::from_raw(a0), message, Some(CapId::from_raw(a2)))?;
            0
        }
        SYS_SEND_BULK => {
            let message = read_message(a1).ok_or(SysError::Fault)?;
            if !in_user_pages(a2, 16) {
                return Err(SysError::Fault);
            }
            // SAFETY: the two words lie in user memory (checked above), which is
            // mapped in the caller's address space, where this syscall runs.
            let (source, len) = unsafe {
                (
                    (a2 as *const u64).read_volatile(),
                    ((a2 + 8) as *const u64).read_volatile(),
                )
            };
            send_bulk(CapId::from_raw(a0), message, source, len)?;
            0
        }
        SYS_IRQ_WAIT => irq_wait(CapId::from_raw(a0))?,
        SYS_RECV_CAP => {
            let (message, transfer) = ipc_recv_transfer(CapId::from_raw(a0))?;
            if !write_received(a1, message, transfer) {
                return Err(SysError::Fault);
            }
            0
        }
        SYS_MAP => {
            map_handle(CapId::from_raw(a0), a1)?;
            0
        }
        SYS_MEM_FREE => {
            free_memory(CapId::from_raw(a0))?;
            0
        }
        _ => return Err(SysError::NoSys),
    })
}

/// The IPC user program. Its first two arguments are the send handle and the
/// reply receive handle. Its third is a 32-byte buffer in the user stack page.
///
/// It writes its message, then checks that a receive-only handle cannot send. It
/// sends a request, receives the reply, and exits with the reply's first word.
/// Any failed step exits with 1.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn user_program() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov r13, rsi",
        "mov r14, rdx",
        "lea rdi, [rip + 3f]",
        "mov esi, {len}",
        "mov eax, {write}",
        "syscall",
        // Fill the request.
        "mov qword ptr [r14], 1",
        "mov qword ptr [r14 + 8], 2",
        "mov qword ptr [r14 + 16], 3",
        "mov qword ptr [r14 + 24], 4",
        // A receive-only handle must be refused by send.
        "mov rdi, r13",
        "mov rsi, r14",
        "mov eax, {send}",
        "syscall",
        // A failed call returns a negated errno, so -4095..=-1 is an error.
        "cmp rax, -4096",
        "jbe 5f",
        // Send the request, then receive the reply into the same buffer.
        "mov rdi, r12",
        "mov rsi, r14",
        "mov eax, {send}",
        "syscall",
        "test rax, rax",
        "jnz 5f",
        "mov rdi, r13",
        "mov rsi, r14",
        "mov eax, {recv}",
        "syscall",
        "test rax, rax",
        "jnz 5f",
        "mov rdi, [r14]",
        "mov eax, {exit}",
        "syscall",
        "5:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "4:",
        "jmp 4b",
        "3:",
        ".ascii \"hello from ring 3\\n\"",
        len = const USER_MESSAGE_LEN,
        write = const SYS_WRITE,
        send = const SYS_SEND,
        recv = const SYS_RECV,
        exit = const SYS_EXIT,
    )
}

/// The process program. Its argument is its number. It stores the number at
/// `PROCESS_DATA`, yields so the other process can store its own number at the
/// same address in its own space, then reads its value back and exits with it.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn process_program() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov rax, {data}",
        "mov qword ptr [rax], r12",
        "mov r13d, {yields}",
        "2:",
        "mov eax, {yield_call}",
        "syscall",
        "dec r13d",
        "jnz 2b",
        "mov rax, {data}",
        "mov rdi, qword ptr [rax]",
        "mov eax, {exit}",
        "syscall",
        "4:",
        "jmp 4b",
        data = const PROCESS_DATA,
        yields = const PROCESS_YIELDS,
        yield_call = const SYS_YIELD,
        exit = const SYS_EXIT,
    )
}

/// The virtual page that the paging check remaps.
const REMAP_VIRT: u64 = 0x7000_0000;

/// Value written through the remapped page.
const REMAP_VALUE: u64 = 0xDEAD_BEEF_0123_4567;

/// Builds the kernel's address spaces, and returns the boot root.
///
/// The boot space has a 4 GiB identity map, one remapped page, and the user code
/// and stack pages with the user bit. The two process spaces are copies of it,
/// each with its own data frame at `PROCESS_DATA`. The boot space is loaded, and
/// the remap is checked with a write through the virtual page.
fn setup_paging() -> Result<u64, &'static str> {
    // SAFETY: called once by the bootstrap thread, before any other thread
    // exists. The pool and the frames are used only here and by the mappings
    // that this function builds.
    unsafe {
        let pool = &mut *addr_of_mut!(PT_POOL);
        let bits = &mut *addr_of_mut!(PT_BITS);
        let frame = addr_of_mut!(REMAP_FRAME) as u64;
        let (code_start, code_end) = (
            core::ptr::addr_of!(__user_code_start) as u64,
            core::ptr::addr_of!(__user_code_end) as u64,
        );
        let (stack_start, stack_end) = user_stack_bounds();
        let mut tables = PageTables::new(pool, bits).map_err(|_| "no page-table pool")?;
        tables
            .identity_map(4)
            .map_err(|_| "cannot identity-map memory")?;
        tables
            .map_4k(REMAP_VIRT, frame, WRITABLE)
            .map_err(|_| "cannot remap the test page")?;
        // The user code and stack pages stay at their physical addresses, but
        // ring 3 may use them.
        for page in (code_start..code_end).step_by(4096) {
            tables
                .map_4k(page, page, USER)
                .map_err(|_| "cannot map user code")?;
        }
        for page in (stack_start..stack_end).step_by(4096) {
            tables
                .map_4k(page, page, USER | WRITABLE)
                .map_err(|_| "cannot map the user stack")?;
        }
        // The local APIC registers are memory-mapped, and must not be cached.
        tables
            .map_4k(apic::APIC_BASE, apic::APIC_BASE, WRITABLE | NO_CACHE)
            .map_err(|_| "cannot map the APIC page")?;
        // The IO-APIC registers are memory-mapped too, and uncached.
        tables
            .map_4k(
                ioapic::IOAPIC_BASE,
                ioapic::IOAPIC_BASE,
                WRITABLE | NO_CACHE,
            )
            .map_err(|_| "cannot map the IO-APIC page")?;
        let boot = tables.root_address_of(0);
        *addr_of_mut!(BOOT_CR3) = boot;

        // Each space copies the boot space. The first two map their own data frame.
        // The last two are for the shared-memory demo.
        let roots = &mut *addr_of_mut!(ROOTS);
        for (process, root_slot) in roots.iter_mut().enumerate() {
            tables
                .select(0)
                .map_err(|_| "cannot select the boot space")?;
            let root = tables.fork().map_err(|_| "cannot fork a process space")?;
            if process < 2 {
                tables
                    .select(root)
                    .map_err(|_| "cannot select a process space")?;
                let data = addr_of_mut!((*addr_of_mut!(PROCESS_FRAMES))[process]) as u64;
                tables
                    .map_4k(PROCESS_DATA, data, USER | WRITABLE)
                    .map_err(|_| "cannot map the process data page")?;
            }
            *root_slot = (root, tables.root_address_of(root));
        }
        tables
            .select(0)
            .map_err(|_| "cannot select the boot space")?;

        // The identity map covers the code, the stacks and the statics that the
        // kernel uses next, in every space, so the switch does not move anything.
        *addr_of_mut!(KERNEL_TABLES) = Some(tables);
        paging::load_root(boot);
        (REMAP_VIRT as *mut u64).write_volatile(REMAP_VALUE);
        if (frame as *const u64).read_volatile() != REMAP_VALUE {
            return Err("remapped page does not reach its frame");
        }
        Ok(boot)
    }
}

/// Maps the physical page `phys` at `vaddr` in address space `root`, and
/// invalidates the translation in case that space is active.
fn map_into_root(root: usize, vaddr: u64, phys: u64, writable: bool) -> Result<(), &'static str> {
    let flags = if writable { USER | WRITABLE } else { USER };
    // SAFETY: called from a syscall, with interrupts off. The kernel tables are
    // used only here and during setup.
    unsafe {
        let tables = (*addr_of_mut!(KERNEL_TABLES))
            .as_mut()
            .ok_or("no kernel tables")?;
        tables
            .select(root)
            .map_err(|_| "cannot select the address space")?;
        let mapped = tables
            .map_4k(vaddr, phys, flags)
            .map_err(|_| "cannot map the page");
        tables
            .select(0)
            .map_err(|_| "cannot select the boot space")?;
        mapped?;
    }
    paging::invlpg(vaddr);
    Ok(())
}

/// The index of the address space whose root has CR3 value `cr3`. Returns `None`
/// for an address space the kernel did not create, so that a syscall cannot map
/// into it.
fn root_index_of(cr3: u64) -> Option<usize> {
    // SAFETY: read only, with interrupts off.
    unsafe {
        if cr3 == *addr_of_mut!(BOOT_CR3) {
            return Some(0);
        }
        let roots = *addr_of_mut!(ROOTS);
        let children = *addr_of_mut!(CHILD_ROOTS);
        roots
            .iter()
            .chain(children.iter())
            .find(|(_, address)| *address == cr3 && *address != 0)
            .map(|(index, _)| *index)
    }
}

/// Maps the shared page at `SHARED_VIRT` in the calling thread's address space.
/// The page is writable if the handle carries WRITE, and read-only otherwise.
fn map_handle(cap: CapId, vaddr: u64) -> Result<(), SysError> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let table = (*addr_of_mut!(CAP_TABLES))[me];
            let object = table.lookup(cap, Rights::READ)?;
            let index = mem_object_index(object).ok_or(SysError::Invalid)?;
            let memory = (*addr_of_mut!(MEM_OBJECTS))[index].ok_or(SysError::BadHandle)?;
            let len = memory.count as u64 * FRAME_SIZE;
            let end = vaddr.checked_add(len).ok_or(SysError::Invalid)?;
            if !vaddr.is_multiple_of(FRAME_SIZE) || vaddr < MEM_WINDOW_START || end > MEM_WINDOW_END
            {
                return Err(SysError::Invalid);
            }
            let writable = table.lookup(cap, Rights::WRITE).is_ok();
            let root = root_index_of((*addr_of_mut!(CONTEXTS))[me].cr3).ok_or(SysError::Invalid)?;
            for (page, &phys) in memory.pages[..memory.count].iter().enumerate() {
                map_into_root(root, vaddr + page as u64 * FRAME_SIZE, phys, writable)
                    .map_err(|_| SysError::NoMemory)?;
            }
            if let Some(object) = (*addr_of_mut!(MEM_OBJECTS))[index].as_mut() {
                object.mapped = true;
            }
            Ok(())
        }
    })
}

/// The memory-object slot an object id names, if it is in range.
fn mem_object_index(object: ObjectId) -> Option<usize> {
    let index = object.0.checked_sub(MEM_OBJECT_BASE)? as usize;
    (index < MAX_MEM_OBJECTS).then_some(index)
}

/// Creates a memory object of `pages` zeroed pages from the frame allocator, and
/// returns a capability for it with all rights in the calling thread's table.
/// Objects are never freed yet.
fn create_memory(pages: u64) -> Result<CapId, SysError> {
    if pages == 0 || pages > MAX_OBJECT_PAGES as u64 {
        return Err(SysError::Invalid);
    }
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // allocator, the object table and its capability table.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let (index, memory) = allocate_object(pages as usize, None)?;
            mint_object(me, index, Rights::ALL, memory).map_err(SysError::from)
        }
    })
}

/// Frees the memory object that `cap` names, and returns its frames.
///
/// Every capability to the object is revoked first, in every table, so no handle
/// can reach the object slot once it is free and may be reused. An object that
/// has been mapped cannot be freed, because nothing unmaps it yet, and object 0,
/// the demo's static page, is never freed.
fn free_memory(cap: CapId) -> Result<(), SysError> {
    const SLOTS: usize = 8;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // capability tables, the object table and the frame allocator.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let object = (*addr_of_mut!(CAP_TABLES))[me].lookup(cap, Rights::WRITE)?;
            let index = mem_object_index(object).ok_or(SysError::Invalid)?;
            if index == 0 {
                return Err(SysError::Invalid);
            }
            let memory = (*addr_of_mut!(MEM_OBJECTS))[index].ok_or(SysError::BadHandle)?;
            if memory.mapped {
                return Err(SysError::Busy);
            }
            let tables = &mut *addr_of_mut!(CAP_TABLES);
            for holder in 0..tables.len() {
                for slot in 0..SLOTS {
                    let Some(handle) = tables[holder].handle_at(slot) else {
                        continue;
                    };
                    if tables[holder].lookup(handle, Rights::NONE) == Ok(object) {
                        // An earlier revoke in this loop may already have removed
                        // this handle's copy, so the result is not checked.
                        let _ = revoke_across(tables, holder, handle);
                    }
                }
            }
            (*addr_of_mut!(MEM_OBJECTS))[index] = None;
            let alloc = (*addr_of_mut!(FRAME_ALLOC))
                .as_mut()
                .ok_or(SysError::NoMemory)?;
            release_pages(alloc, &memory.pages[..memory.count]);
            Ok(())
        }
    })
}

/// Allocates an object of `count` zeroed pages, with `payload` copied in from the
/// start if given. The payload is `(address, length)`, and the caller has checked
/// that it lies in user memory and fits. The object is not stored yet: see
/// [`mint_object`]. Must be called with interrupts off.
///
/// # Safety
///
/// Interrupts must be off, so nothing else uses the allocator or the table.
unsafe fn allocate_object(
    count: usize,
    payload: Option<(u64, usize)>,
) -> Result<(usize, MemObject), SysError> {
    // SAFETY: as stated above.
    unsafe {
        let index = (*addr_of_mut!(MEM_OBJECTS))
            .iter()
            .position(|slot| slot.is_none())
            .ok_or(SysError::Full)?;
        let alloc = (*addr_of_mut!(FRAME_ALLOC))
            .as_mut()
            .ok_or(SysError::NoMemory)?;
        let mut memory = MemObject {
            pages: [0; MAX_OBJECT_PAGES],
            count,
            mapped: false,
        };
        for page in 0..count {
            match alloc.allocate() {
                Some(frame) => memory.pages[page] = frame.start(),
                None => {
                    release_pages(alloc, &memory.pages[..page]);
                    return Err(SysError::NoMemory);
                }
            }
        }
        for &phys in &memory.pages[..count] {
            for offset in (0..FRAME_SIZE).step_by(8) {
                ((phys + offset) as *mut u64).write_volatile(0);
            }
        }
        if let Some((source, len)) = payload {
            for offset in 0..len {
                let byte = ((source + offset as u64) as *const u8).read_volatile();
                let page = memory.pages[offset / FRAME_SIZE as usize];
                ((page + offset as u64 % FRAME_SIZE) as *mut u8).write_volatile(byte);
            }
        }
        Ok((index, memory))
    }
}

/// Stores `memory` in slot `index`, and mints a capability for it with `rights`
/// in thread `thread`'s table. On failure the frames are returned.
///
/// # Safety
///
/// Interrupts must be off, and `index` must be the slot `allocate_object` chose.
unsafe fn mint_object(
    thread: usize,
    index: usize,
    rights: Rights,
    memory: MemObject,
) -> Result<CapId, CapError> {
    // SAFETY: as stated above.
    unsafe {
        let object = ObjectId(MEM_OBJECT_BASE + index as u32);
        match (*addr_of_mut!(CAP_TABLES))[thread].mint(object, rights) {
            Ok(cap) => {
                (*addr_of_mut!(MEM_OBJECTS))[index] = Some(memory);
                Ok(cap)
            }
            Err(error) => {
                if let Some(alloc) = (*addr_of_mut!(FRAME_ALLOC)).as_mut() {
                    release_pages(alloc, &memory.pages[..memory.count]);
                }
                Err(error)
            }
        }
    }
}

/// Whether `address..address + len` lies in the user code or stack pages.
fn in_user_pages(address: u64, len: u64) -> bool {
    let (code_start, code_end) = (
        core::ptr::addr_of!(__user_code_start) as u64,
        core::ptr::addr_of!(__user_code_end) as u64,
    );
    let (stack_start, stack_end) = user_stack_bounds();
    within(code_start, code_end, address, len) || within(stack_start, stack_end, address, len)
}

/// Sends `message` on `channel`, with a bulk payload: `len` bytes copied from
/// user memory at `source` into a new memory object, which the receiver gets as
/// a capability. The copy is taken at send time, so the sender may reuse its
/// buffer at once. The sender's temporary handle to the object is revoked when
/// the send completes, so the receiver's copy is the only one.
fn send_bulk(channel: CapId, message: Message, source: u64, len: u64) -> Result<(), SysError> {
    if len == 0 || len > MAX_OBJECT_PAGES as u64 * FRAME_SIZE {
        return Err(SysError::Invalid);
    }
    if !in_user_pages(source, len) {
        return Err(SysError::Fault);
    }
    let count = len.div_ceil(FRAME_SIZE) as usize;
    let temporary = interrupts::without(|| {
        // SAFETY: interrupts are off, as for `create_memory`.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let (index, memory) = allocate_object(count, Some((source, len as usize)))?;
            mint_object(me, index, Rights::ALL, memory).map_err(SysError::from)
        }
    })?;
    let sent = ipc_send_transfer(channel, message, Some(temporary));
    interrupts::without(|| {
        // SAFETY: interrupts are off. The thread's own table is used only here.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let _ = (*addr_of_mut!(CAP_TABLES))[me].revoke(temporary);
        }
    });
    sent
}

/// Returns frames to the allocator. The addresses come from `allocate`, so the
/// frame constructor cannot fail for them.
fn release_pages(alloc: &mut FrameAllocator<'static>, pages: &[u64]) {
    for &phys in pages {
        if let Some(frame) = PhysFrame::from_address(phys) {
            let _ = alloc.free(frame);
        }
    }
}

/// Gives the processor to the next ready thread. Returns `true` if it switched.
fn yield_now() -> bool {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this is the only code using the table.
        // The other threads' stacks are statics that were set up before their
        // first switch.
        unsafe { (*addr_of_mut!(SCHED)).yield_now() }
    })
}

/// Timer tick. Acknowledges the line, counts the tick, and preempts the running
/// thread if another one is ready.
fn on_tick() {
    apic::eoi();
    TICKS.fetch_add(1, Ordering::Relaxed);
    if yield_now() {
        PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Called by the scheduler before each switch, with the incoming thread's slot.
/// Points syscalls and ring-3 interrupts at its kernel stack, and loads its
/// address space if it differs from the current one.
fn install_context(slot: usize) {
    // SAFETY: called with interrupts off, by the scheduler, during a switch.
    let context = unsafe { (*addr_of_mut!(CONTEXTS))[slot] };
    syscall::set_kernel_stack(context.kstack_top);
    if paging::read_cr3() != context.cr3 {
        // SAFETY: every context's root maps the kernel's code, data and stacks
        // (each space is a copy of the boot space), so execution continues.
        unsafe { paging::load_root(context.cr3) };
    }
}

/// Spawns a thread on `stack` and records its context.
fn spawn_thread(stack: &mut [u64], entry: u64, cr3: u64) -> Result<usize, &'static str> {
    let kstack_top = stack.as_ptr() as u64 + stack.len() as u64 * 8;
    // SAFETY: the thread is not running yet, and interrupts are off, so nothing
    // else touches the table.
    let slot = unsafe { (*addr_of_mut!(SCHED)).spawn(stack, entry) }
        .map_err(|_| "could not start a thread")?;
    // SAFETY: as above.
    unsafe {
        (*addr_of_mut!(CONTEXTS))[slot] = Context { kstack_top, cr3 };
    }
    Ok(slot)
}

/// One worker: prints `letter` three times, spinning and yielding between
/// prints, then exits. Two of these run side by side. The output is tagged, so
/// the test can pick the letters out of the other threads' output.
fn worker(letter: u8) -> ! {
    interrupts::enable();
    for _ in 0..3 {
        // The tag is written with interrupts off, so a tick cannot split it and
        // let the other worker's tag land inside it.
        interrupts::without(|| {
            serial::write_byte(b'<');
            serial::write_byte(letter);
            serial::write_byte(b'>');
        });
        for _ in 0..SPIN {
            core::hint::spin_loop();
        }
        yield_now();
    }
    finish()
}

/// Marks the running demo thread finished and exits it.
fn finish() -> ! {
    FINISHED.fetch_add(1, Ordering::Relaxed);
    interrupts::without(|| {
        // SAFETY: interrupts are off, and the thread is the running one. It exits
        // for good here.
        unsafe { (*addr_of_mut!(SCHED)).exit_current() }
    });
    qemu::halt_forever()
}

/// Sends `message` on the endpoint named by `cap`, blocking if the queue has no
/// receiver.
fn ipc_send(cap: CapId, message: Message) -> Result<(), SysError> {
    ipc_send_transfer(cap, message, None)
}

/// Sends `message` on the endpoint named by `cap`, with `transfer` moved to the
/// receiver. The transfer is a handle of the sender, which needs `GRANT`. If a
/// receiver is already waiting, the copy is installed in its table now, so the
/// receiver gets it atomically with the message. Otherwise the sender stays
/// blocked with its handle, and the receiver copies it when it takes the message.
fn ipc_send_transfer(cap: CapId, message: Message, transfer: Transfer) -> Result<(), SysError> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // kernel tables. The table is copied, so the sender's and the receiver's
        // tables are not borrowed together. The references end before the block,
        // and the thread that resumes takes its own.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let table = (*addr_of_mut!(CAP_TABLES))[me];
            let object = table.lookup(cap, Rights::WRITE)?;
            if let Some(handle) = transfer {
                table.lookup(handle, Rights::GRANT)?;
            }
            let endpoint = (*addr_of_mut!(ENDPOINTS))
                .get_mut(object.0 as usize)
                .ok_or(SysError::Invalid)?;
            let waiting = endpoint.next_receiver();
            let stored = match (transfer, waiting) {
                (Some(handle), Some(receiver)) => Some(install_from(me, handle, receiver)?),
                (other, _) => other,
            };
            let sched = &mut *addr_of_mut!(SCHED);
            let outcome = match endpoint.send_transfer(sched.current(), message, stored) {
                Ok(outcome) => outcome,
                Err(_) => {
                    // The endpoint is full: undo the copy installed above, so the
                    // receiver keeps no handle for a message it never got.
                    if let (Some(installed), Some(receiver)) = (stored, waiting) {
                        let _ = (*addr_of_mut!(CAP_TABLES))[receiver].revoke(installed);
                    }
                    return Err(SysError::Full);
                }
            };
            match outcome {
                SendOutcome::Delivered { receiver } => {
                    sched.wake(receiver);
                    Ok(())
                }
                SendOutcome::Blocked => {
                    if sched.block_current() {
                        Ok(())
                    } else {
                        Err(SysError::Deadlock)
                    }
                }
            }
        }
    })
}

/// Receives a message from the endpoint named by `cap`, blocking until one is
/// sent.
fn ipc_recv(cap: CapId) -> Result<Message, SysError> {
    ipc_recv_transfer(cap).map(|(message, _)| message)
}

/// Copies handle `handle` of thread `from` into thread `to`, with the same
/// rights, and returns the new handle. The copy records its origin, so it is
/// revoked with the original. Must be called with interrupts off.
fn install_from(from: usize, handle: CapId, to: usize) -> Result<CapId, SysError> {
    // SAFETY: interrupts are off, so this is the only code using the tables. The
    // source is copied, so the two tables are not borrowed at once.
    unsafe {
        let source = (*addr_of_mut!(CAP_TABLES))[from];
        let rights = source.rights_of(handle)?;
        Ok(grant(
            &source,
            from,
            handle,
            &mut (*addr_of_mut!(CAP_TABLES))[to],
            rights,
        )?)
    }
}

/// Receives a message and the capability sent with it, if any. The capability
/// is copied into the receiver's table, and its new handle is returned.
fn ipc_recv_transfer(cap: CapId) -> Result<(Message, Transfer), SysError> {
    interrupts::without(|| {
        // SAFETY: as for `ipc_send`.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let object = (*addr_of_mut!(CAP_TABLES))[me].lookup(cap, Rights::READ)?;
            let endpoint = (*addr_of_mut!(ENDPOINTS))
                .get_mut(object.0 as usize)
                .ok_or(SysError::Invalid)?;
            let sched = &mut *addr_of_mut!(SCHED);
            match endpoint
                .receive(sched.current())
                .map_err(|_| SysError::Full)?
            {
                RecvOutcome::Received {
                    message,
                    sender,
                    transfer,
                } => {
                    // The sender must wake even if the install fails: its message
                    // is already taken.
                    let installed = transfer
                        .map(|handle| install_from(sender, handle, me))
                        .transpose();
                    sched.wake(sender);
                    Ok((message, installed?))
                }
                RecvOutcome::Blocked => {
                    if !sched.block_current() {
                        return Err(SysError::Deadlock);
                    }
                    // The sender installed the copy before waking us.
                    endpoint
                        .take_delivery_transfer(sched.current())
                        .ok_or(SysError::Invalid)
                }
            }
        }
    })
}

/// The server's handles. Panics the run if the bootstrap thread did not set them.
fn server_handles() -> [CapId; 2] {
    // SAFETY: the bootstrap thread wrote this before the server thread ran, and
    // nothing writes it afterwards.
    let handles = unsafe { *addr_of_mut!(SERVER_HANDLES) };
    handles.unwrap_or_else(|| fail("server handles missing"))
}

/// Copies handle `handle` of the running thread into the table of thread slot
/// `target`, with `rights` (a raw mask: 1 is READ, 2 is WRITE, 4 is GRANT). Returns
/// the new handle's raw value, which is valid in the target's table.
fn grant_handle(handle: CapId, target: u64, rights: u64) -> Result<u64, SysError> {
    let rights = Rights::from_bits(rights).ok_or(SysError::Invalid)?;
    let target = usize::try_from(target).map_err(|_| SysError::Invalid)?;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables. The source is copied, so the two tables are not borrowed at once.
        unsafe {
            let sched = &*addr_of_mut!(SCHED);
            if target >= MAX_THREADS || sched.threads()[target].state == State::Free {
                return Err(SysError::Invalid);
            }
            let me = sched.current();
            let source = (*addr_of_mut!(CAP_TABLES))[me];
            let copy = grant(
                &source,
                me,
                handle,
                &mut (*addr_of_mut!(CAP_TABLES))[target],
                rights,
            )?;
            Ok(copy.raw())
        }
    })
}

/// Revokes handle `handle` of the running thread, together with every copy made
/// from it, in any table. Memory objects that nothing refers to any more are
/// then reclaimed.
fn revoke_handle(handle: CapId) -> Result<(), SysError> {
    interrupts::without(|| -> Result<(), SysError> {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            revoke_across(&mut *addr_of_mut!(CAP_TABLES), me, handle)?;
        }
        Ok(())
    })?;
    reclaim_unreferenced_objects();
    Ok(())
}

/// Lets go of every capability the running thread holds, as it exits. Each
/// handle is deleted without revoking anything else, so copies that other threads
/// hold stay valid. Then the memory objects nothing refers to are reclaimed.
fn release_handles() {
    const SLOTS: usize = 8;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // capability tables.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let tables = &mut *addr_of_mut!(CAP_TABLES);
            for slot in 0..SLOTS {
                if let Some(handle) = tables[me].handle_at(slot) {
                    let _ = delete_across(tables, me, handle);
                }
            }
        }
    });
    reclaim_unreferenced_objects();
}

/// Frees each memory object that is not mapped and that no thread's table holds a
/// handle to, and returns its frames. Object 0, the demo's static page, is never
/// freed. Must be called from a thread, not from an interrupt.
fn reclaim_unreferenced_objects() {
    const SLOTS: usize = 8;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // object table, the capability tables and the frame allocator.
        unsafe {
            let tables = &*addr_of_mut!(CAP_TABLES);
            for index in 1..MAX_MEM_OBJECTS {
                let Some(memory) = (*addr_of_mut!(MEM_OBJECTS))[index] else {
                    continue;
                };
                if memory.mapped {
                    continue;
                }
                let object = ObjectId(MEM_OBJECT_BASE + index as u32);
                let referenced = tables.iter().any(|table| {
                    (0..SLOTS).any(|slot| {
                        table
                            .handle_at(slot)
                            .is_some_and(|handle| table.lookup(handle, Rights::NONE) == Ok(object))
                    })
                });
                if referenced {
                    continue;
                }
                (*addr_of_mut!(MEM_OBJECTS))[index] = None;
                if let Some(alloc) = (*addr_of_mut!(FRAME_ALLOC)).as_mut() {
                    release_pages(alloc, &memory.pages[..memory.count]);
                }
            }
        }
    });
}

/// Checks the grant path through the syscall dispatcher. The bootstrap thread
/// mints a root handle, grants a read-only copy to `target`, and then checks
/// that the copy reads and does not write, that it cannot be granted onward, and
/// that a grant to a free slot is refused.
fn grant_self_test(target: usize) -> bool {
    // SAFETY: called by the bootstrap thread before the demo threads run.
    let root = match unsafe { (*addr_of_mut!(CAP_TABLES))[0].mint(ObjectId(2), Rights::ALL) } {
        Ok(handle) => handle,
        Err(_) => return false,
    };
    let read_raw = syscall_dispatch(SYS_GRANT, root.raw(), target as u64, 1);
    if is_error(read_raw) {
        return false;
    }
    let read = CapId::from_raw(read_raw);
    // SAFETY: as above.
    let (reads, no_write) = unsafe {
        let tables = &*addr_of_mut!(CAP_TABLES);
        (
            tables[target].lookup(read, Rights::READ) == Ok(ObjectId(2)),
            tables[target].lookup(read, Rights::WRITE) == Err(CapError::Denied),
        )
    };
    // The copy has no GRANT, so the target cannot grant it onward. The check is
    // made in the target's own table, where the copy's handle is valid.
    // SAFETY: as above. The source is copied, so the two tables are not borrowed
    // at once.
    let onward_refused = unsafe {
        let source = (*addr_of_mut!(CAP_TABLES))[target];
        let other = &mut (*addr_of_mut!(CAP_TABLES))[MAX_THREADS - 1];
        grant(&source, target, read, other, Rights::READ) == Err(CapError::NotGrantable)
    };
    // Slot 7 is never used by the demo, so it is free.
    let free_refused = syscall_dispatch(SYS_GRANT, root.raw(), MAX_THREADS as u64 - 1, 1)
        == SysError::Invalid.code();
    // Revoking the root also revokes the copy that the target holds.
    let revoked = syscall_dispatch(SYS_REVOKE, root.raw(), 0, 0) == 0;
    // SAFETY: as above.
    let gone = unsafe { (*addr_of_mut!(CAP_TABLES))[target].lookup(read, Rights::READ) }
        == Err(CapError::InvalidHandle);
    reads && no_write && onward_refused && free_refused && revoked && gone
}

/// Free frames in the allocator, or `None` before it exists.
fn free_frame_count() -> Option<usize> {
    // SAFETY: called by the bootstrap thread, before the demo threads run.
    unsafe {
        (*addr_of_mut!(FRAME_ALLOC))
            .as_ref()
            .map(|alloc| alloc.free_frames())
    }
}

/// Creates a two-page memory object and frees it. Checks that the frames come
/// back to the allocator, and that the handle no longer maps anything.
fn mem_free_self_test() -> bool {
    let (Some(before), handle) = (
        free_frame_count(),
        syscall_dispatch(SYS_MEM_CREATE, 2, 0, 0),
    ) else {
        return false;
    };
    if is_error(handle) || free_frame_count() != Some(before - 2) {
        return false;
    }
    if syscall_dispatch(SYS_MEM_FREE, handle, 0, 0) != 0 {
        return false;
    }
    let returned = free_frame_count() == Some(before);
    let stale =
        syscall_dispatch(SYS_MAP, handle, MEM_WINDOW_START, 0) == SysError::BadHandle.code();
    returned && stale
}

/// Creates a memory object and revokes its only handle. Checks that the object's
/// frames come back, so the revoke reclaimed it.
fn reclaim_self_test() -> bool {
    let Some(before) = free_frame_count() else {
        return false;
    };
    let handle = syscall_dispatch(SYS_MEM_CREATE, 1, 0, 0);
    if is_error(handle) {
        return false;
    }
    if syscall_dispatch(SYS_REVOKE, handle, 0, 0) != 0 {
        return false;
    }
    free_frame_count() == Some(before)
}

/// Prints the live capability graph, one line per capability, between
/// `caps: begin` and `caps: end`. Each line names the table and slot, the object,
/// the rights as letters (R, W, G, or `-`), the parent slot in the same table, and
/// the origin as `table.slot` for a copy. `tpt-k-build caps` reads these lines.
fn dump_capabilities() {
    const SLOTS: usize = 8;
    let mut out = serial::Writer;
    let _ = writeln!(out, "caps: begin");
    // SAFETY: called by the bootstrap thread before the demo threads run.
    let tables = unsafe { &*addr_of_mut!(CAP_TABLES) };
    for (table_index, table) in tables.iter().enumerate() {
        for slot in 0..SLOTS {
            let Some(handle) = table.handle_at(slot) else {
                continue;
            };
            let Ok(object) = table.lookup(handle, Rights::NONE) else {
                continue;
            };
            let rights = table.rights_of(handle).unwrap_or(Rights::NONE);
            let _ = write!(
                out,
                "cap: table={table_index} slot={slot} object={} rights=",
                object.0
            );
            for (right, letter) in [
                (Rights::READ, 'R'),
                (Rights::WRITE, 'W'),
                (Rights::GRANT, 'G'),
            ] {
                let _ = write!(out, "{}", if rights.contains(right) { letter } else { '-' });
            }
            let _ = write!(out, " parent=");
            match table.parent_at(slot) {
                Some(parent) => {
                    let _ = write!(out, "{}", parent.index());
                }
                None => {
                    let _ = write!(out, "-");
                }
            }
            let _ = write!(out, " origin=");
            match table.origin_at(slot) {
                Some(origin) => {
                    let _ = write!(out, "{}.{}", origin.table, origin.handle.index());
                }
                None => {
                    let _ = write!(out, "-");
                }
            }
            let _ = writeln!(out);
        }
    }
    let _ = writeln!(out, "caps: end");
}

/// Checks that failed calls return distinct negated errno values, for three
/// different causes: a handle that was never valid, an unknown call number, and
/// a bad argument.
fn errno_self_test() -> bool {
    syscall_dispatch(SYS_MAP, 0xdead_beef, 0, 0) == SysError::BadHandle.code()
        && syscall_dispatch(999, 0, 0, 0) == SysError::NoSys.code()
        && syscall_dispatch(SYS_MEM_CREATE, 0, 0, 0) == SysError::Invalid.code()
        && SysError::BadHandle.code() != SysError::NoSys.code()
        && is_error(SysError::NoSys.code())
        && !is_error(0)
}

/// Reads the loader's memory map and builds the frame allocator from it. Checks
/// that the kernel image is not free, and that a frame can be taken and returned.
fn check_memory(handoff: u64) -> Result<(), &'static str> {
    // SAFETY: the loader wrote the record at `handoff` and its map, both in
    // identity-mapped memory that stays valid until the kernel reclaims it.
    let info = unsafe { bootmap::read(handoff) }.ok_or("no boot information")?;
    // SAFETY: the bitmap is used only here, before any other frame user exists.
    let bits: &'static mut [u64] = unsafe { &mut *addr_of_mut!(FRAME_BITS) };
    // SAFETY: as for `read` above.
    let mut frames: FrameAllocator<'static> =
        unsafe { bootmap::allocator_from_map(&info, COVERED_FRAMES, bits) }
            .ok_or("cannot build the frame allocator")?;
    let image = PhysFrame::from_address(0x10_0000).ok_or("bad kernel address")?;
    if !frames.is_allocated(image) {
        return Err("the kernel image frame is free");
    }
    let free = frames.free_frames();
    let frame = frames.allocate().ok_or("no free frame")?;
    frames.free(frame).map_err(|_| "cannot return a frame")?;
    let mut out = serial::Writer;
    let _ = writeln!(out, "memory: {free} free frames");
    report_firmware(&info, &mut out);
    // SAFETY: called once, by the bootstrap thread, before any other thread
    // exists. The allocator is used only from syscalls after this point.
    unsafe { *addr_of_mut!(FRAME_ALLOC) = Some(frames) };
    Ok(())
}

/// Prints the firmware facts the loader passed on: the ACPI RSDP and the
/// framebuffer. Neither is used yet; the RSDP feeds `tpt-k-hal-acpi` (M3).
fn report_firmware(info: &bootmap::BootInfo, out: &mut serial::Writer) {
    if info.rsdp_phys == 0 {
        let _ = writeln!(out, "handoff: no acpi rsdp");
    } else {
        let _ = writeln!(out, "handoff: rsdp at {:#x}", info.rsdp_phys);
    }
    if info.fb_format == bootmap::pixel_format::NONE {
        let _ = writeln!(out, "handoff: no framebuffer");
    } else {
        let _ = writeln!(
            out,
            "handoff: framebuffer {}x{} stride {} format {} at {:#x}",
            info.fb_width, info.fb_height, info.fb_stride, info.fb_format, info.fb_base
        );
    }
}

/// Prints `message` and ends the run as a failure.
fn fail(message: &str) -> ! {
    serial::write_str(message);
    serial::write_str("\n");
    qemu::exit(qemu::EXIT_FAILURE)
}

fn server() -> ! {
    interrupts::enable();
    let [request_recv, reply_send] = server_handles();
    let request = ipc_recv(request_recv).unwrap_or_else(|e| fail(e.as_str()));
    let mut out = serial::Writer;
    let _ = writeln!(
        out,
        "ipc: server got {} {} {} {}",
        request[0], request[1], request[2], request[3]
    );
    let sum: u64 = request.iter().sum();
    ipc_send(reply_send, [sum, 0, 0, 0]).unwrap_or_else(|e| fail(e.as_str()));
    finish()
}

/// Enters ring 3 with the arguments set up by the bootstrap thread. Does not
/// return: the program leaves through `exit`, and the syscall handler ends the
/// thread.
fn user_thread() -> ! {
    // SAFETY: the bootstrap thread set USER_ENTRY before spawning this thread,
    // and nothing writes it afterwards.
    let spec = unsafe { *addr_of_mut!(USER_ENTRY) }.unwrap_or_else(|| fail("user entry missing"));
    // SAFETY: the entry is in the mapped user code pages, the stack is the user
    // stack page, and the scheduler has installed this thread's kernel stack.
    unsafe { syscall::enter_user(spec.entry, spec.stack_top, spec.a0, spec.a1, spec.a2) }
}

/// Enters ring 3 as user process `index`, with its number as the argument.
fn process(index: usize) -> ! {
    // SAFETY: the bootstrap thread set the entry before spawning the process
    // thread, and nothing writes it afterwards.
    let spec = unsafe { (*addr_of_mut!(PROCESS_ENTRY))[index] }
        .unwrap_or_else(|| fail("process entry missing"));
    // SAFETY: as for `user_thread`. The process's address space is installed
    // when the scheduler switches to this thread.
    unsafe {
        syscall::enter_user(
            spec.entry,
            spec.stack_top,
            index as u64 + 1,
            spec.a1,
            spec.a2,
        )
    }
}

extern "C" fn worker_a() -> ! {
    worker(b'A')
}

extern "C" fn worker_b() -> ! {
    worker(b'B')
}

extern "C" fn server_entry() -> ! {
    server()
}

extern "C" fn user_thread_entry() -> ! {
    user_thread()
}

/// Ring-3 exception handler. The faulting thread is ended, and the kernel goes
/// on running the other threads. The kernel itself still stops on its own faults.
fn user_fault(vector: u64, rip: u64, error: u64) -> ! {
    let mut out = serial::Writer;
    let _ = writeln!(
        out,
        "user: fault vector {vector} at rip {rip:#x} error {error:#x}"
    );
    release_handles();
    finish()
}

/// A user program that reads address 0, which no user mapping covers. The read
/// faults in ring 3. The `ud2` after it is not reached; if it were, it would
/// fault too, with another vector, and the smoke test would notice.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn fault_program() -> ! {
    naked_asm!("xor eax, eax", "mov rax, [rax]", "ud2")
}

/// Enters ring 3 at [`fault_program`], to be killed by the user fault handler.
fn fault_thread() -> ! {
    let stack_top = core::ptr::addr_of_mut!(USER_STACK) as u64 + 4096;
    // SAFETY: the entry is in the mapped user code pages, the stack is the user
    // stack page, and the scheduler has installed this thread's kernel stack.
    unsafe {
        syscall::enter_user(
            fault_program as *const () as usize as u64,
            stack_top,
            0,
            0,
            0,
        )
    }
}

extern "C" fn fault_thread_entry() -> ! {
    fault_thread()
}

extern "C" fn process0_entry() -> ! {
    process(0)
}

extern "C" fn process1_entry() -> ! {
    process(1)
}

/// Zero-copy sender. Arguments: the memory capability, the channel send handle,
/// and a buffer whose first word is the receiver's slot. It maps the shared page,
/// writes the marker, grants a read-only copy of the memory capability to the
/// receiver, and sends that handle over the channel.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn shm_sender() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov r13, rsi",
        "mov r14, rdx",
        "mov rdi, r12",
        "mov rsi, {shared}",
        "mov eax, {map}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rax, {shared}",
        "mov qword ptr [rax], {marker}",
        "mov rdi, r12",
        "mov rsi, qword ptr [r14]",
        "mov edx, 1",
        "mov eax, {grant}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        "mov qword ptr [r14], rax",
        "mov rdi, r13",
        "mov rsi, r14",
        "mov eax, {send}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov edi, 0",
        "mov eax, {exit}",
        "syscall",
        "9:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        shared = const SHARED_VIRT,
        marker = const SHM_MARKER,
        map = const SYS_MAP,
        grant = const SYS_GRANT,
        send = const SYS_SEND,
        exit = const SYS_EXIT,
    )
}

/// Zero-copy receiver. Arguments: the channel receive handle and a buffer. It
/// receives the read-only handle, maps the shared page with it, and exits with
/// the value the sender wrote. The page was never copied.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn shm_receiver() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov r14, rsi",
        "mov rdi, r12",
        "mov rsi, r14",
        "mov eax, {recv}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rdi, qword ptr [r14]",
        "mov rsi, {shared}",
        "mov eax, {map}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rax, {shared}",
        "mov rdi, qword ptr [rax]",
        "mov eax, {exit}",
        "syscall",
        "9:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        shared = const SHARED_VIRT,
        map = const SYS_MAP,
        recv = const SYS_RECV,
        exit = const SYS_EXIT,
    )
}

/// Enters ring 3 with the arguments of `spec`.
fn enter_spec(spec: UserEntry) -> ! {
    // SAFETY: the zero-copy entries name mapped user pages, and the scheduler has
    // installed this thread's kernel stack and address space.
    unsafe { syscall::enter_user(spec.entry, spec.stack_top, spec.a0, spec.a1, spec.a2) }
}

extern "C" fn shm_sender_entry() -> ! {
    // SAFETY: set by the bootstrap thread before the threads run.
    let spec =
        unsafe { (*addr_of_mut!(SHM_ENTRY))[0] }.unwrap_or_else(|| fail("sender entry missing"));
    enter_spec(spec)
}

extern "C" fn shm_receiver_entry() -> ! {
    // SAFETY: as above.
    let spec =
        unsafe { (*addr_of_mut!(SHM_ENTRY))[1] }.unwrap_or_else(|| fail("receiver entry missing"));
    enter_spec(spec)
}

/// Creates the zero-copy demo's capabilities in the sender's and receiver's
/// tables: the memory object (all rights) and the channel, with send for the
/// sender and receive for the receiver.
fn grant_shared_memory(
    sender: usize,
    receiver: usize,
) -> Result<(CapId, CapId, CapId), &'static str> {
    // SAFETY: called by the bootstrap thread before the demo threads run.
    unsafe {
        let tables = &mut *addr_of_mut!(CAP_TABLES);
        // The shared demo page is memory object 0.
        (*addr_of_mut!(MEM_OBJECTS))[0] = Some(MemObject {
            pages: [addr_of_mut!(SHARED_FRAME) as u64, 0, 0, 0],
            count: 1,
            mapped: false,
        });
        let memory = tables[0]
            .mint(MEMORY_OBJECT, Rights::ALL)
            .map_err(|_| "cannot mint the memory capability")?;
        let channel = tables[0]
            .mint(CHANNEL_OBJECT, Rights::ALL)
            .map_err(|_| "cannot mint the channel capability")?;
        let source = tables[0];
        let sender_memory = grant(&source, 0, memory, &mut tables[sender], Rights::ALL)
            .map_err(|_| "cannot grant the memory capability")?;
        let sender_send = grant(&source, 0, channel, &mut tables[sender], Rights::WRITE)
            .map_err(|_| "cannot grant the channel send handle")?;
        let receiver_recv = grant(&source, 0, channel, &mut tables[receiver], Rights::READ)
            .map_err(|_| "cannot grant the channel receive handle")?;
        Ok((sender_memory, sender_send, receiver_recv))
    }
}

/// The bulk payload the root thread sends. The child checks its first word, 0x5a.
#[unsafe(link_section = ".user")]
static BULK_PAYLOAD: [u8; 16] = [0x5a, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

/// Where the child maps the bulk object: a window address clear of the first
/// object, which is at the window start.
const BULK_WINDOW: u64 = MEM_WINDOW_START + 0x1_0000;

/// Length of the program image that `SPAWN_IMAGE` holds.
const IMAGE_LEN: usize = 14;

/// A program image for `SYS_SPAWN_IMAGE`. It is position independent and uses
/// only the exit syscall: `mov edi, 7; mov eax, 2; syscall; jmp $`. The root
/// thread passes it to the kernel from its own code pages, and the child exits
/// with 7.
#[unsafe(link_section = ".user")]
static SPAWN_IMAGE: [u8; IMAGE_LEN] = [
    0xBF, 0x07, 0x00, 0x00, 0x00, // mov edi, 7
    0xB8, 0x02, 0x00, 0x00, 0x00, // mov eax, SYS_EXIT
    0x0F, 0x05, // syscall
    0xEB, 0xFE, // jmp $
];
const _: () = assert!(SYS_EXIT == 2);

/// Root thread. Its arguments: the channel handle with all rights, the channel
/// send handle, and a buffer holding the message. It spawns program 0 with the
/// all-rights handle, narrowed to READ, and then sends the message. Then it
/// spawns a child from `SPAWN_IMAGE`, with READ on the same channel, which
/// exits with 7.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn root_program() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov r13, rsi",
        "mov r14, rdx",
        "mov edi, 0",
        "mov rsi, r12",
        "mov edx, 1",
        "mov eax, {spawn}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        // A two-page memory object, mapped at the window, with 0x5a written to
        // its second page. Its capability goes to the child with the message.
        "mov edi, 2",
        "mov eax, {mem_create}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        "mov r15, rax",
        "mov rdi, r15",
        "mov rsi, {window}",
        "mov eax, {map}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rcx, {second_page}",
        "mov rdx, 0x5a",
        "mov qword ptr [rcx], rdx",
        // Send the message and the capability to the child.
        "mov rdi, r13",
        "mov rsi, r14",
        "mov rdx, r15",
        "mov eax, {send_cap}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        // A bulk message: word 0 of the message is the payload's length. The
        // payload descriptor (address, length) is pushed so that rsp points at it.
        "mov qword ptr [r14], 16",
        "push 16",
        "lea rax, [rip + {bulk}]",
        "push rax",
        "mov rdi, r13",
        "mov rsi, r14",
        "mov rdx, rsp",
        "mov eax, {send_bulk}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        // The image descriptor, pushed so that rsp points at it: the image
        // address, its length, the handle, and the rights (READ).
        "push 1",
        "push r12",
        "push {len}",
        "lea rax, [rip + {image}]",
        "push rax",
        "mov rdi, rsp",
        "mov eax, {spawn_image}",
        "syscall",
        "cmp rax, -4096",
        "ja 9f",
        // Exit with the value read back from the object's second page.
        "mov rcx, {second_page}",
        "mov rdi, qword ptr [rcx]",
        "mov eax, {exit}",
        "syscall",
        "9:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        spawn = const SYS_SPAWN,
        spawn_image = const SYS_SPAWN_IMAGE,
        mem_create = const SYS_MEM_CREATE,
        map = const SYS_MAP,
        window = const MEM_WINDOW_START,
        second_page = const MEM_WINDOW_START + FRAME_SIZE,
        send_cap = const SYS_SEND_CAP,
        send_bulk = const SYS_SEND_BULK,
        bulk = sym BULK_PAYLOAD,
        exit = const SYS_EXIT,
        len = const IMAGE_LEN,
        image = sym SPAWN_IMAGE,
    )
}

/// Spawned child. Its arguments: a read-only channel handle and a buffer. It
/// receives one message and exits with its first word.
#[unsafe(naked)]
#[unsafe(link_section = ".user")]
extern "C" fn child_program() -> ! {
    naked_asm!(
        "mov r12, rdi",
        "mov r14, rsi",
        // The first message carries a memory capability. Keep its word 0 in r13,
        // then map the object at the window and check the root's value in its
        // second page.
        "mov rdi, r12",
        "mov rsi, r14",
        "mov eax, {recv_cap}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov r13, qword ptr [r14]",
        "mov r15, qword ptr [r14 + 32]",
        "cmp r15, -1",
        "je 9f",
        "mov rdi, r15",
        "mov rsi, {window}",
        "mov eax, {map}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rcx, {second}",
        "mov rax, qword ptr [rcx]",
        "cmp rax, 0x5a",
        "jne 9f",
        // The second message carries a bulk object. Map it at the bulk window
        // and check the payload's first word.
        "mov rdi, r12",
        "mov rsi, r14",
        "mov eax, {recv_cap}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov r15, qword ptr [r14 + 32]",
        "cmp r15, -1",
        "je 9f",
        "mov rdi, r15",
        "mov rsi, {bulk_window}",
        "mov eax, {map}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rcx, {bulk_window}",
        "mov rax, qword ptr [rcx]",
        "cmp rax, 0x5a",
        "jne 9f",
        "mov rdi, r13",
        "mov eax, {exit}",
        "syscall",
        "9:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        recv_cap = const SYS_RECV_CAP,
        map = const SYS_MAP,
        window = const MEM_WINDOW_START,
        second = const MEM_WINDOW_START + FRAME_SIZE,
        bulk_window = const BULK_WINDOW,
        exit = const SYS_EXIT,
    )
}

/// Enters ring 3 as the root thread.
extern "C" fn root_thread_entry() -> ! {
    // SAFETY: set by the bootstrap thread before the thread runs.
    let spec = unsafe { *addr_of_mut!(ROOT_ENTRY) }.unwrap_or_else(|| fail("root entry missing"));
    enter_spec(spec)
}

/// Enters ring 3 as the child in spawn slot `index`.
fn spawned(index: usize) -> ! {
    // SAFETY: the spawn operation sets the entry before the child's thread runs.
    let spec = unsafe { (*addr_of_mut!(SPAWN_ENTRY))[index] }
        .unwrap_or_else(|| fail("spawned entry missing"));
    enter_spec(spec)
}

extern "C" fn spawned_0() -> ! {
    spawned(0)
}

extern "C" fn spawned_1() -> ! {
    spawned(1)
}

/// Creates a child thread for program `program`, in a new address space (a copy
/// of the boot space), and gives it a read-only copy of `handle` from the caller.
/// The child starts at its program's entry, and gets the copy as its first
/// argument. Returns the child's thread slot.
fn spawn_child(program: u64, handle: CapId, rights: u64) -> Result<usize, SysError> {
    let rights = Rights::from_bits(rights).ok_or(SysError::Invalid)?;
    if program != 0 {
        return Err(SysError::Invalid);
    }
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables and the spawn state.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let index = (*addr_of_mut!(SPAWN_USED))
                .iter()
                .position(|used| !used)
                .ok_or(SysError::Full)?;
            let tables = (*addr_of_mut!(KERNEL_TABLES))
                .as_mut()
                .ok_or(SysError::NoMemory)?;
            tables.select(0).map_err(|_| SysError::NoMemory)?;
            let root = tables.fork().map_err(|_| SysError::NoMemory)?;
            let cr3 = tables.root_address_of(root);
            (*addr_of_mut!(CHILD_ROOTS))[index] = (root, cr3);

            let stack = &mut (*addr_of_mut!(SPAWN_STACKS))[index].0;
            let entry = if index == 0 {
                spawned_0 as *const () as usize as u64
            } else {
                spawned_1 as *const () as usize as u64
            };
            let slot = spawn_thread(stack, entry, cr3).map_err(|_| SysError::Full)?;
            (*addr_of_mut!(SPAWN_USED))[index] = true;

            let source = (*addr_of_mut!(CAP_TABLES))[me];
            let copy = grant(
                &source,
                me,
                handle,
                &mut (*addr_of_mut!(CAP_TABLES))[slot],
                rights,
            )
            .map_err(SysError::from)?;
            (*addr_of_mut!(SPAWN_ENTRY))[index] = Some(UserEntry {
                entry: child_program as *const () as usize as u64,
                stack_top: user_stack_bounds().1,
                a0: copy.raw(),
                a1: user_stack_bounds().1 - 3584,
                a2: 0,
            });
            let mut out = serial::Writer;
            let _ = writeln!(out, "spawn: child in slot {slot}");
            Ok(slot)
        }
    })
}

/// Spawns a child from a program image in the caller's memory.
///
/// `descriptor` is the address of four words in the caller's user pages: the
/// image address, the image length, the handle to give the child, and its
/// rights. The image must lie in the user code pages, and be at most one page.
/// The kernel copies it into the slot's image page, maps that page at
/// [`IMAGE_BASE`] in the child's own address space only, and starts the child
/// there with the narrowed handle as its first argument. Returns the spawn slot.
fn spawn_image(descriptor: u64) -> Result<usize, SysError> {
    let (code_start, code_end) = (
        core::ptr::addr_of!(__user_code_start) as u64,
        core::ptr::addr_of!(__user_code_end) as u64,
    );
    let (stack_start, stack_end) = user_stack_bounds();
    let in_user_pages = |address: u64, len: u64| {
        within(code_start, code_end, address, len) || within(stack_start, stack_end, address, len)
    };
    if !in_user_pages(descriptor, 32) {
        return Err(SysError::Fault);
    }
    let mut words = [0u64; 4];
    for (index, word) in words.iter_mut().enumerate() {
        // SAFETY: the descriptor lies inside the caller's user pages (checked
        // above), which are mapped and readable in the caller's address space,
        // and this syscall runs in that space.
        *word = unsafe { ((descriptor + index as u64 * 8) as *const u64).read_volatile() };
    }
    let [image, length, handle, rights_bits] = words;
    if length == 0 || length > IMAGE_MAX || !within(code_start, code_end, image, length) {
        return Err(SysError::Fault);
    }
    let rights = Rights::from_bits(rights_bits).ok_or(SysError::Invalid)?;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables, the image pages and the spawn state.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let index = (*addr_of_mut!(SPAWN_USED))
                .iter()
                .position(|used| !used)
                .ok_or(SysError::Full)?;
            let tables = (*addr_of_mut!(KERNEL_TABLES))
                .as_mut()
                .ok_or(SysError::NoMemory)?;
            tables.select(0).map_err(|_| SysError::NoMemory)?;
            let root = tables.fork().map_err(|_| SysError::NoMemory)?;
            let cr3 = tables.root_address_of(root);
            (*addr_of_mut!(CHILD_ROOTS))[index] = (root, cr3);

            // Copy the image into the slot's page, then map that page into the
            // child only. The identity mapping makes the page writable here.
            let page = addr_of_mut!((*addr_of_mut!(IMAGE_PAGES))[index].0) as u64;
            for offset in 0..length {
                // SAFETY: the source range lies in the user code pages (checked
                // above); the destination is the first `length` bytes of the
                // page, and `length` is at most one page.
                let byte = ((image + offset) as *const u8).read_volatile();
                ((page + offset) as *mut u8).write_volatile(byte);
            }
            tables.select(root).map_err(|_| SysError::NoMemory)?;
            tables
                .map_4k(IMAGE_BASE, page, USER)
                .map_err(|_| SysError::NoMemory)?;
            tables.select(0).map_err(|_| SysError::NoMemory)?;

            let stack = &mut (*addr_of_mut!(SPAWN_STACKS))[index].0;
            let entry = if index == 0 {
                spawned_0 as *const () as usize as u64
            } else {
                spawned_1 as *const () as usize as u64
            };
            let slot = spawn_thread(stack, entry, cr3).map_err(|_| SysError::Full)?;
            (*addr_of_mut!(SPAWN_USED))[index] = true;

            let source = (*addr_of_mut!(CAP_TABLES))[me];
            let copy = grant(
                &source,
                me,
                CapId::from_raw(handle),
                &mut (*addr_of_mut!(CAP_TABLES))[slot],
                rights,
            )
            .map_err(SysError::from)?;
            (*addr_of_mut!(SPAWN_ENTRY))[index] = Some(UserEntry {
                entry: IMAGE_BASE,
                stack_top: stack_end,
                a0: copy.raw(),
                a1: 0,
                a2: 0,
            });
            let mut out = serial::Writer;
            let _ = writeln!(out, "spawn: image child in slot {slot}");
            Ok(slot)
        }
    })
}

/// Gives the root thread (slot `root`) the channel with all rights, and a send
/// handle. Returns both handles.
fn grant_root_channel(root: usize) -> Result<(CapId, CapId), &'static str> {
    // SAFETY: called by the bootstrap thread before the demo threads run.
    unsafe {
        let tables = &mut *addr_of_mut!(CAP_TABLES);
        let channel = tables[0]
            .mint(ROOT_CHANNEL_OBJECT, Rights::ALL)
            .map_err(|_| "cannot mint the root channel")?;
        let source = tables[0];
        let all = grant(&source, 0, channel, &mut tables[root], Rights::ALL)
            .map_err(|_| "cannot grant the root channel")?;
        let send = grant(&source, 0, channel, &mut tables[root], Rights::WRITE)
            .map_err(|_| "cannot grant the root send handle")?;
        Ok((all, send))
    }
}

/// Size of the boot stack in bytes.
const STACK_SIZE: usize = 64 * 1024;

/// The boot stack. It lives in `.bss`, which the loader zeroes.
#[repr(C, align(16))]
struct BootStack([u8; STACK_SIZE]);

static mut BOOT_STACK: BootStack = BootStack([0; STACK_SIZE]);

/// Entry point, placed at the ELF entry address by the linker script.
///
/// # Safety
///
/// Called once by the boot loader. It only sets `rsp` and calls `kmain`, so it
/// needs no valid stack of its own.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn _start() -> ! {
    naked_asm!(
        "cli",
        "lea rsp, [rip + {stack}]",
        "add rsp, {size}",
        "and rsp, -16",
        "call {kmain}",
        "ud2",
        stack = sym BOOT_STACK,
        size = const STACK_SIZE,
        kmain = sym kmain,
    )
}

/// Gives each thread its handles, in its own table. The user thread's table holds
/// a root for each endpoint, derived into a send handle and receive handles. The
/// server's table holds a receive handle for the request and a send handle for
/// the reply. The process tables stay empty.
fn grant_handles(user: usize, server: usize) -> Result<(), &'static str> {
    // SAFETY: called by the bootstrap thread before any demo thread runs.
    unsafe {
        let tables = &mut *addr_of_mut!(CAP_TABLES);
        let user_table = &mut tables[user];
        let request = user_table
            .mint(ObjectId(0), Rights::ALL)
            .map_err(|_| "cannot mint capability")?;
        let reply = user_table
            .mint(ObjectId(1), Rights::ALL)
            .map_err(|_| "cannot mint capability")?;
        let request_send = user_table
            .derive(request, Rights::WRITE)
            .map_err(|_| "cannot derive capability")?;
        let request_read = user_table
            .derive(request, Rights::READ)
            .map_err(|_| "cannot derive capability")?;
        let reply_read = user_table
            .derive(reply, Rights::READ)
            .map_err(|_| "cannot derive capability")?;
        *addr_of_mut!(USER_HANDLES) = Some([request_send, reply_read, request_read]);

        let server_table = &mut tables[server];
        let server_request = server_table
            .mint(ObjectId(0), Rights::READ)
            .map_err(|_| "cannot mint capability")?;
        let server_reply = server_table
            .mint(ObjectId(1), Rights::WRITE)
            .map_err(|_| "cannot mint capability")?;
        *addr_of_mut!(SERVER_HANDLES) = Some([server_request, server_reply]);
    }
    Ok(())
}

/// Checks that a handle from the user thread's table is valid there and refused
/// by a process table.
fn tables_are_isolated(user: usize, process: usize) -> bool {
    // SAFETY: called by the bootstrap thread before the threads run.
    unsafe {
        let tables = &*addr_of_mut!(CAP_TABLES);
        let Some(handles) = *addr_of_mut!(USER_HANDLES) else {
            return false;
        };
        let handle = handles[0];
        tables[user].lookup(handle, Rights::WRITE).is_ok()
            && tables[process].lookup(handle, Rights::WRITE) == Err(CapError::InvalidHandle)
    }
}

/// First Rust code on the boot stack. Sets up the segments, the exception table,
/// the address spaces, the timer and the syscall boundary, then runs the demo.
extern "sysv64" fn kmain(handoff: u64) -> ! {
    serial::init();
    if let Err(message) = check_memory(handoff) {
        fail(message);
    }
    gdt::load();
    idt::load();
    idt::set_user_fault_handler(user_fault);
    let boot_root = match setup_paging() {
        Ok(root) => root,
        Err(message) => fail(message),
    };
    serial::write_str("paging: remap ok\n");
    pic::init();
    apic::enable();
    #[cfg(feature = "replay")]
    let ticks = REPLAY_TIMER_COUNTS;
    #[cfg(not(feature = "replay"))]
    let ticks = apic::calibrate_10ms();
    if ticks == 0 || ticks == u32::MAX {
        fail("apic timer calibration failed");
    }
    // One period of 10 ms gives a 100 Hz tick.
    apic::start_periodic(idt::IRQ_BASE, ticks);
    idt::set_irq_handler(0, on_tick);
    let mut out = serial::Writer;
    let _ = writeln!(out, "apic: timer {ticks} counts per 10 ms");
    serial::write_str("Hello tpt-k\n");

    // SAFETY: `int3` raises vector 3. The handler resumes at the next
    // instruction, so this has no effect on the code after it.
    unsafe { asm!("int3") };
    if let Err(message) = check_ioapic_rtc() {
        fail(message);
    }
    // The check left interrupts on. The bootstrap thread sets up the demo with
    // them off, so no demo thread runs before the final enable below.
    interrupts::disable();

    let boot_top = core::ptr::addr_of_mut!(BOOT_STACK) as u64 + STACK_SIZE as u64;
    // SAFETY: the bootstrap thread is the only code touching these statics until
    // the threads exist.
    unsafe {
        (*addr_of_mut!(CONTEXTS))[0] = Context {
            kstack_top: boot_top,
            cr3: boot_root,
        };
        (*addr_of_mut!(SCHED)).set_switch_hook(install_context);
    }
    syscall::init(syscall_dispatch);

    let user_stack_top = core::ptr::addr_of_mut!(USER_STACK) as u64 + 4096;
    let process_entry = UserEntry {
        entry: process_program as *const () as usize as u64,
        stack_top: user_stack_top,
        a0: 0,
        a1: 0,
        a2: 0,
    };
    // SAFETY: as above.
    unsafe {
        (*addr_of_mut!(PROCESS_ENTRY)) = [Some(process_entry), Some(process_entry)];
    }

    let roots = unsafe { *addr_of_mut!(ROOTS) };
    let threads: [(*mut [u64], u64, u64); 8] = unsafe {
        [
            (
                addr_of_mut!((*addr_of_mut!(STACK_A)).0) as *mut [u64],
                worker_a as *const () as usize as u64,
                boot_root,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_B)).0) as *mut [u64],
                worker_b as *const () as usize as u64,
                boot_root,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_SERVER)).0) as *mut [u64],
                server_entry as *const () as usize as u64,
                boot_root,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_USER)).0) as *mut [u64],
                user_thread_entry as *const () as usize as u64,
                boot_root,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_PROCESS0)).0) as *mut [u64],
                process0_entry as *const () as usize as u64,
                roots[0].1,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_PROCESS1)).0) as *mut [u64],
                process1_entry as *const () as usize as u64,
                roots[1].1,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_SHM_SENDER)).0) as *mut [u64],
                shm_sender_entry as *const () as usize as u64,
                roots[2].1,
            ),
            (
                addr_of_mut!((*addr_of_mut!(STACK_SHM_RECEIVER)).0) as *mut [u64],
                shm_receiver_entry as *const () as usize as u64,
                roots[3].1,
            ),
        ]
    };
    let mut slots = [0usize; 8];
    for (slot, (stack, entry, cr3)) in slots.iter_mut().zip(threads) {
        // SAFETY: each stack is a distinct static, handed over to its thread here.
        let stack = unsafe { &mut *stack };
        *slot = spawn_thread(stack, entry, cr3).unwrap_or_else(|e| fail(e));
    }
    // Spawn order fixes the slots: the server is third, the user thread fourth,
    // and the processes fifth and sixth. The grants depend on it.
    let (server_slot, user_slot) = (slots[2], slots[3]);
    // The zero-copy demo: the sender gets the memory capability and the send
    // handle, and the receiver gets the receive handle. The sender's buffer names
    // the receiver's slot. The buffers differ from the IPC thread's, because the
    // threads share the user stack page.
    // The root thread spawns a child, and sends it a message. The message is
    // written in the buffer at offset 1536 of the user stack page.
    // SAFETY: the bootstrap thread hands this stack to the root thread.
    let root_stack = unsafe { &mut (*addr_of_mut!(STACK_ROOT)).0 };
    let root_slot = spawn_thread(
        root_stack,
        root_thread_entry as *const () as usize as u64,
        boot_root,
    )
    .unwrap_or_else(|e| fail(e));
    let (root_all, root_send) = grant_root_channel(root_slot).unwrap_or_else(|e| fail(e));
    let root_buffer = user_stack_top - 2560;
    if !write_message(root_buffer, [5, 6, 7, 8]) {
        fail("cannot prepare the root buffer");
    }
    // SAFETY: as above; the root thread reads this when it first runs.
    unsafe {
        *addr_of_mut!(ROOT_ENTRY) = Some(UserEntry {
            entry: root_program as *const () as usize as u64,
            stack_top: user_stack_top,
            a0: root_all.raw(),
            a1: root_send.raw(),
            a2: root_buffer,
        });
    }
    // The driver thread holds the RTC's interrupt object. It waits there, and
    // the RTC is started only now, so the boot check's interrupts are not counted.
    let driver_stack = unsafe { &mut (*addr_of_mut!(STACK_DRIVER)).0 };
    let driver_slot = spawn_thread(
        driver_stack,
        driver_thread_entry as *const () as usize as u64,
        boot_root,
    )
    .unwrap_or_else(|e| fail(e));
    // A thread that faults in ring 3 is ended by the fault handler, not the kernel.
    let fault_stack = unsafe { &mut (*addr_of_mut!(STACK_FAULT)).0 };
    spawn_thread(
        fault_stack,
        fault_thread_entry as *const () as usize as u64,
        boot_root,
    )
    .unwrap_or_else(|e| fail(e));
    // SAFETY: the bootstrap thread is the only code touching the table and the
    // entry before the threads run.
    let irq_cap = unsafe {
        (*addr_of_mut!(CAP_TABLES))[driver_slot]
            .mint(IRQ_OBJECT, Rights::READ)
            .unwrap_or_else(|_| fail("cannot mint the interrupt capability"))
    };
    unsafe {
        *addr_of_mut!(DRIVER_ENTRY) = Some(UserEntry {
            entry: driver_program as *const () as usize as u64,
            stack_top: user_stack_top,
            a0: irq_cap.raw(),
            a1: 0,
            a2: 0,
        });
    }
    IRQ_PENDING.store(0, Ordering::Relaxed);
    rtc::start_periodic(rtc::RATE_2_HZ);
    let (sender_slot, receiver_slot) = (slots[6], slots[7]);
    let (memory, sender_channel, receiver_channel) =
        grant_shared_memory(sender_slot, receiver_slot).unwrap_or_else(|e| fail(e));
    let sender_buffer = user_stack_top - 1024;
    let receiver_buffer = user_stack_top - 512;
    if !write_message(sender_buffer, [receiver_slot as u64, 0, 0, 0]) {
        fail("cannot prepare the sender buffer");
    }
    // SAFETY: as above; the zero-copy threads read these when they first run.
    unsafe {
        (*addr_of_mut!(SHM_ENTRY)) = [
            Some(UserEntry {
                entry: shm_sender as *const () as usize as u64,
                stack_top: user_stack_top,
                a0: memory.raw(),
                a1: sender_channel.raw(),
                a2: sender_buffer,
            }),
            Some(UserEntry {
                entry: shm_receiver as *const () as usize as u64,
                stack_top: user_stack_top,
                a0: receiver_channel.raw(),
                a1: receiver_buffer,
                a2: 0,
            }),
        ];
    }
    grant_handles(user_slot, server_slot).unwrap_or_else(|e| fail(e));
    if !tables_are_isolated(user_slot, slots[4]) {
        fail("capability tables are not isolated");
    }
    serial::write_str("handles: process tables are isolated\n");
    if !grant_self_test(slots[5]) {
        fail("grant check failed");
    }
    serial::write_str("handles: grant attenuates rights\n");
    if !errno_self_test() {
        fail("error codes are wrong");
    }
    serial::write_str("errno: failures are negated errno values\n");
    if !mem_free_self_test() {
        fail("memory free check failed");
    }
    serial::write_str("mem: freed object returned its frames\n");
    if !reclaim_self_test() {
        fail("revoking the last handle did not reclaim the object");
    }
    serial::write_str("mem: revoking the last handle reclaimed the object\n");

    // SAFETY: as above; the user thread reads this when it first runs.
    unsafe {
        let handles = (*addr_of_mut!(USER_HANDLES)).unwrap_or_else(|| fail("user handles missing"));
        *addr_of_mut!(USER_ENTRY) = Some(UserEntry {
            entry: user_program as *const () as usize as u64,
            stack_top: user_stack_top,
            a0: handles[0].raw(),
            a1: handles[1].raw(),
            a2: user_stack_top - 2048,
        });
    }

    dump_capabilities();
    interrupts::enable();
    while FINISHED.load(Ordering::Relaxed) < DEMO_THREADS {
        yield_now();
    }
    let mut out = serial::Writer;
    let _ = writeln!(
        out,
        "\npreempt: ticks={} timer switches={}",
        TICKS.load(Ordering::Relaxed),
        PREEMPTIONS.load(Ordering::Relaxed)
    );
    serial::write_str("scheduler: ok\n");
    qemu::exit(qemu::EXIT_SUCCESS)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut out = serial::Writer;
    let _ = match info.location() {
        Some(location) => writeln!(
            out,
            "kernel panic at {}:{}",
            location.file(),
            location.line()
        ),
        None => writeln!(out, "kernel panic"),
    };
    qemu::exit(qemu::EXIT_FAILURE)
}
