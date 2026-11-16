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
use tpt_k_kernel_arch_x86_64::{apic, gdt, idt, interrupts, pic, qemu, serial, syscall};
use tpt_k_kernel_ipc::cap::{grant, revoke_across, CapError, CapTable};
use tpt_k_kernel_ipc::endpoint::{Endpoint, Message, RecvOutcome, SendOutcome};
use tpt_k_kernel_ipc::{CapId, ObjectId, Rights};
use tpt_k_kernel_mem::bootmap;
use tpt_k_kernel_mem::{Frame as PhysFrame, FrameAllocator};
use tpt_k_kernel_sched::{Scheduler, State, MAX_THREADS};

/// Words per kernel thread stack (4 KiB).
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
static mut ENDPOINTS: [Endpoint<4>; 3] = [Endpoint::new(); 3];

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

/// The root thread's stack and entry.
static mut STACK_ROOT: UserThreadStack = UserThreadStack([0; 2048]);
static mut ROOT_ENTRY: Option<UserEntry> = None;

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

/// Virtual address where the shared page is mapped. Both demo threads use it.
const SHARED_VIRT: u64 = 0x0080_0000;
/// Value the sender writes to the shared page.
const SHM_MARKER: u64 = 0x00C0_FFEE;
/// Object ids: endpoints 0 to 2, then the memory object.
const CHANNEL_OBJECT: ObjectId = ObjectId(2);
const MEMORY_OBJECT: ObjectId = ObjectId(3);

/// Longest buffer a `write` call may name.
const MAX_WRITE: u64 = 256;

/// Size of the message buffer the user program uses (four words).
const MESSAGE_BYTES: u64 = 32;

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
/// the user thread, and two processes.
const DEMO_THREADS: usize = 10;

/// Demo threads that have finished.
static FINISHED: AtomicUsize = AtomicUsize::new(0);

/// Timer ticks seen.
static TICKS: AtomicUsize = AtomicUsize::new(0);

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

/// The syscall handler. `number` selects the call, and `a0`, `a1`, `a2` are its
/// arguments. A failed call returns `u64::MAX`.
extern "C" fn syscall_dispatch(number: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    match number {
        SYS_WRITE => {
            // The message lives in the user code pages, and the copy must stay
            // inside them.
            let start = core::ptr::addr_of!(__user_code_start) as u64;
            let end = core::ptr::addr_of!(__user_code_end) as u64;
            if a1 > MAX_WRITE || !within(start, end, a0, a1) {
                return u64::MAX;
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
            finish()
        }
        SYS_SEND => {
            let Some(message) = read_message(a1) else {
                return u64::MAX;
            };
            match ipc_send(CapId::from_raw(a0), message) {
                Ok(()) => 0,
                Err(_) => u64::MAX,
            }
        }
        SYS_RECV => match ipc_recv(CapId::from_raw(a0)) {
            Ok(message) => {
                if write_message(a1, message) {
                    0
                } else {
                    u64::MAX
                }
            }
            Err(_) => u64::MAX,
        },
        SYS_YIELD => {
            yield_now();
            0
        }
        SYS_GRANT => grant_handle(CapId::from_raw(a0), a1, a2).unwrap_or(u64::MAX),
        SYS_REVOKE => match revoke_handle(CapId::from_raw(a0)) {
            Ok(()) => 0,
            Err(_) => u64::MAX,
        },
        SYS_SPAWN => match spawn_child(a0, CapId::from_raw(a1), a2) {
            Ok(slot) => slot as u64,
            Err(_) => u64::MAX,
        },
        SYS_MAP => match map_handle(CapId::from_raw(a0), a1) {
            Ok(()) => 0,
            Err(_) => u64::MAX,
        },
        _ => u64::MAX,
    }
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
        "cmp rax, -1",
        "jne 5f",
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
fn map_handle(cap: CapId, vaddr: u64) -> Result<(), &'static str> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let table = (*addr_of_mut!(CAP_TABLES))[me];
            let object = table
                .lookup(cap, Rights::READ)
                .map_err(|_| "map capability refused")?;
            if object != MEMORY_OBJECT {
                return Err("not a memory object");
            }
            if vaddr != SHARED_VIRT {
                return Err("map address outside the shared window");
            }
            let writable = table.lookup(cap, Rights::WRITE).is_ok();
            let root =
                root_index_of((*addr_of_mut!(CONTEXTS))[me].cr3).ok_or("unknown address space")?;
            let phys = addr_of_mut!(SHARED_FRAME) as u64;
            map_into_root(root, vaddr, phys, writable)
        }
    })
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
        serial::write_byte(b'<');
        serial::write_byte(letter);
        serial::write_byte(b'>');
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
fn ipc_send(cap: CapId, message: Message) -> Result<(), &'static str> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // kernel tables. The references end before the block, and the thread
        // that resumes takes its own.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let object = (*addr_of_mut!(CAP_TABLES))[me]
                .lookup(cap, Rights::WRITE)
                .map_err(|_| "send capability refused")?;
            let endpoint = (*addr_of_mut!(ENDPOINTS))
                .get_mut(object.0 as usize)
                .ok_or("no such endpoint")?;
            let sched = &mut *addr_of_mut!(SCHED);
            match endpoint
                .send(sched.current(), message)
                .map_err(|_| "endpoint full")?
            {
                SendOutcome::Delivered { receiver } => {
                    sched.wake(receiver);
                    Ok(())
                }
                SendOutcome::Blocked => {
                    if sched.block_current() {
                        Ok(())
                    } else {
                        Err("send would deadlock")
                    }
                }
            }
        }
    })
}

/// Receives a message from the endpoint named by `cap`, blocking until one is
/// sent.
fn ipc_recv(cap: CapId) -> Result<Message, &'static str> {
    interrupts::without(|| {
        // SAFETY: as for `ipc_send`.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let object = (*addr_of_mut!(CAP_TABLES))[me]
                .lookup(cap, Rights::READ)
                .map_err(|_| "receive capability refused")?;
            let endpoint = (*addr_of_mut!(ENDPOINTS))
                .get_mut(object.0 as usize)
                .ok_or("no such endpoint")?;
            let sched = &mut *addr_of_mut!(SCHED);
            match endpoint
                .receive(sched.current())
                .map_err(|_| "endpoint full")?
            {
                RecvOutcome::Received { message, sender } => {
                    sched.wake(sender);
                    Ok(message)
                }
                RecvOutcome::Blocked => {
                    if !sched.block_current() {
                        return Err("receive would deadlock");
                    }
                    endpoint
                        .take_delivery(sched.current())
                        .ok_or("woken without a message")
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
fn grant_handle(handle: CapId, target: u64, rights: u64) -> Option<u64> {
    let rights = Rights::from_bits(rights)?;
    let target = usize::try_from(target).ok()?;
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables. The source is copied, so the two tables are not borrowed at once.
        unsafe {
            let sched = &*addr_of_mut!(SCHED);
            if target >= MAX_THREADS || sched.threads()[target].state == State::Free {
                return None;
            }
            let me = sched.current();
            let source = (*addr_of_mut!(CAP_TABLES))[me];
            let copy = grant(
                &source,
                me,
                handle,
                &mut (*addr_of_mut!(CAP_TABLES))[target],
                rights,
            )
            .ok()?;
            Some(copy.raw())
        }
    })
}

/// Revokes handle `handle` of the running thread, together with every copy made
/// from it, in any table.
fn revoke_handle(handle: CapId) -> Result<(), CapError> {
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            revoke_across(&mut *addr_of_mut!(CAP_TABLES), me, handle)
        }
    })
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
    if read_raw == u64::MAX {
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
    let free_refused =
        syscall_dispatch(SYS_GRANT, root.raw(), MAX_THREADS as u64 - 1, 1) == u64::MAX;
    // Revoking the root also revokes the copy that the target holds.
    let revoked = syscall_dispatch(SYS_REVOKE, root.raw(), 0, 0) == 0;
    // SAFETY: as above.
    let gone = unsafe { (*addr_of_mut!(CAP_TABLES))[target].lookup(read, Rights::READ) }
        == Err(CapError::InvalidHandle);
    reads && no_write && onward_refused && free_refused && revoked && gone
}

/// Reads the loader's memory map and builds the frame allocator from it. Checks
/// that the kernel image is not free, and that a frame can be taken and returned.
fn check_memory(handoff: u64) -> Result<(), &'static str> {
    // SAFETY: the loader wrote the record at `handoff` and its map, both in
    // identity-mapped memory that stays valid until the kernel reclaims it.
    let info = unsafe { bootmap::read(handoff) }.ok_or("no boot information")?;
    // SAFETY: the bitmap is used only here, before any other frame user exists.
    let bits = unsafe { &mut *addr_of_mut!(FRAME_BITS) };
    // SAFETY: as for `read` above.
    let mut frames: FrameAllocator<'_> =
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
    Ok(())
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
    let request = ipc_recv(request_recv).unwrap_or_else(|e| fail(e));
    let mut out = serial::Writer;
    let _ = writeln!(
        out,
        "ipc: server got {} {} {} {}",
        request[0], request[1], request[2], request[3]
    );
    let sum: u64 = request.iter().sum();
    ipc_send(reply_send, [sum, 0, 0, 0]).unwrap_or_else(|e| fail(e));
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
        "cmp rax, -1",
        "je 9f",
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

/// Root thread. Its arguments: the channel handle with all rights, the channel
/// send handle, and a buffer holding the message. It spawns program 0 with the
/// all-rights handle, narrowed to READ, and then sends the message.
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
        "cmp rax, -1",
        "je 9f",
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
        spawn = const SYS_SPAWN,
        send = const SYS_SEND,
        exit = const SYS_EXIT,
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
        "mov rdi, r12",
        "mov rsi, r14",
        "mov eax, {recv}",
        "syscall",
        "test rax, rax",
        "jnz 9f",
        "mov rdi, qword ptr [r14]",
        "mov eax, {exit}",
        "syscall",
        "9:",
        "mov edi, 1",
        "mov eax, {exit}",
        "syscall",
        "8:",
        "jmp 8b",
        recv = const SYS_RECV,
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
fn spawn_child(program: u64, handle: CapId, rights: u64) -> Result<usize, &'static str> {
    let rights = Rights::from_bits(rights).ok_or("bad rights")?;
    if program != 0 {
        return Err("no such program");
    }
    interrupts::without(|| {
        // SAFETY: interrupts are off, so this thread is the only one using the
        // tables and the spawn state.
        unsafe {
            let me = (*addr_of_mut!(SCHED)).current();
            let index = (*addr_of_mut!(SPAWN_USED))
                .iter()
                .position(|used| !used)
                .ok_or("no free spawn stack")?;
            let tables = (*addr_of_mut!(KERNEL_TABLES))
                .as_mut()
                .ok_or("no kernel tables")?;
            tables
                .select(0)
                .map_err(|_| "cannot select the boot space")?;
            let root = tables
                .fork()
                .map_err(|_| "cannot fork a space for the child")?;
            let cr3 = tables.root_address_of(root);
            (*addr_of_mut!(CHILD_ROOTS))[index] = (root, cr3);

            let stack = &mut (*addr_of_mut!(SPAWN_STACKS))[index].0;
            let entry = if index == 0 {
                spawned_0 as *const () as usize as u64
            } else {
                spawned_1 as *const () as usize as u64
            };
            let slot = spawn_thread(stack, entry, cr3)?;
            (*addr_of_mut!(SPAWN_USED))[index] = true;

            let source = (*addr_of_mut!(CAP_TABLES))[me];
            let copy = grant(
                &source,
                me,
                handle,
                &mut (*addr_of_mut!(CAP_TABLES))[slot],
                rights,
            )
            .map_err(|_| "cannot grant the child its handle")?;
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

/// Gives the root thread (slot `root`) the channel with all rights, and a send
/// handle. Returns both handles.
fn grant_root_channel(root: usize) -> Result<(CapId, CapId), &'static str> {
    // SAFETY: called by the bootstrap thread before the demo threads run.
    unsafe {
        let tables = &mut *addr_of_mut!(CAP_TABLES);
        let channel = tables[0]
            .mint(CHANNEL_OBJECT, Rights::ALL)
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
    let boot_root = match setup_paging() {
        Ok(root) => root,
        Err(message) => fail(message),
    };
    serial::write_str("paging: remap ok\n");
    pic::init();
    apic::enable();
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
