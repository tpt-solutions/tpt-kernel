// SPDX-License-Identifier: MIT OR Apache-2.0
//! The ring-3 boundary: `SYSCALL`/`SYSRET` entry, and the transfer into user
//! mode.
//!
//! A user program runs with `rax` = syscall number and `rdi`, `rsi`, `rdx` as
//! arguments. `syscall` lands in `syscall_entry`, which switches to the current
//! thread's kernel stack and calls the handler registered with [`init`]. The
//! handler's result comes back in `rax`.
//!
//! The entry keeps the user stack pointer on the kernel stack, so a syscall can
//! block and another thread can make its own syscalls meanwhile. The kernel
//! stack is per thread: the scheduler calls [`set_kernel_stack`] whenever it
//! switches threads.

use core::arch::{asm, naked_asm};

use crate::gdt::{USER_CODE, USER_DATA};
use crate::tss;

/// A syscall handler: number, then three arguments, and the result.
pub type Handler = extern "C" fn(u64, u64, u64, u64) -> u64;

const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_FMASK: u32 = 0xC000_0084;
/// EFER bit that enables `syscall` and `sysret`.
const EFER_SCE: u64 = 1;
/// Interrupt flag. It is masked on syscall entry, so the entry code runs with
/// interrupts off until it has switched stacks.
const RFLAGS_IF: u64 = 1 << 9;

// Words the entry code reads. `SCRATCH` holds the user stack pointer for the
// few instructions between `syscall` and the stack switch, and no other code
// runs in that window, because interrupts are off.
static mut HANDLER: u64 = 0;
static mut SCRATCH: u64 = 0;
static mut STACK_TOP: u64 = 0;

/// Reads a model-specific register.
///
/// # Safety
///
/// `msr` must be a register that exists on this CPU. Reading it has no side
/// effects here.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    // SAFETY: the caller guarantees `msr` exists. `rdmsr` writes only the
    // output registers.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high,
             options(nomem, nostack, preserves_flags));
    }
    (u64::from(high) << 32) | u64::from(low)
}

/// Writes a model-specific register.
///
/// # Safety
///
/// `msr` must be a register that exists on this CPU, and `value` must be valid
/// for it. Writes can change how the CPU enters and leaves the kernel.
unsafe fn wrmsr(msr: u32, value: u64) {
    // SAFETY: the caller guarantees the register and the value (see the function
    // contract).
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32,
             options(nomem, nostack, preserves_flags));
    }
}

/// Enables `syscall` and routes it to `handler`. Call once, from the bootstrap
/// thread, after the GDT and the TSS are loaded. Then call [`set_kernel_stack`]
/// for the first thread.
pub fn init(handler: Handler) {
    // SAFETY: ring 0, called once before any user code runs. The MSRs exist on
    // every x86_64 CPU. STAR selects the kernel code and data segments for
    // `syscall`, and the user segments for `sysret`: the user code selector is
    // base + 16 and the user data selector is base + 8, with base 0x10 here.
    // FMASK clears IF on entry.
    unsafe {
        wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_SCE);
        wrmsr(
            MSR_STAR,
            (0x10u64 << 48) | (u64::from(crate::gdt::KERNEL_CODE) << 32),
        );
        wrmsr(MSR_LSTAR, syscall_entry as *const () as usize as u64);
        wrmsr(MSR_FMASK, RFLAGS_IF);
        core::ptr::addr_of_mut!(HANDLER).write(handler as *const () as usize as u64);
    }
}

/// Sets the kernel stack that syscalls and ring-3 interrupts use, which is the
/// stack of the thread that is about to run. `top` is the first address past a
/// 16-byte aligned stack.
pub fn set_kernel_stack(top: u64) {
    // SAFETY: called by the scheduler with interrupts off, when it hands the
    // processor to a thread. Nothing reads the word except the entry code, which
    // runs with interrupts off too.
    unsafe {
        core::ptr::addr_of_mut!(STACK_TOP).write(top);
    }
    tss::set_kernel_stack(top);
}

/// The `syscall` landing point. Switches to the kernel stack, calls the
/// handler, and returns to the user program with `sysretq`.
///
/// # Safety
///
/// Reached only through `syscall` from ring 3, with the MSRs set by [`init`].
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
        "mov [{scratch}], rsp",
        "mov rsp, [{stack_top}]",
        "push qword ptr [{scratch}]",
        "push rcx",
        "push r11",
        // Three words are pushed, so rsp is 8 mod 16. This makes it 16-aligned
        // for the call.
        "sub rsp, 8",
        // Syscall arguments: rax = number, rdi, rsi, rdx. The handler takes
        // (number, a0, a1, a2) in rdi, rsi, rdx, rcx.
        "mov rcx, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, rax",
        "call qword ptr [{handler}]",
        "add rsp, 8",
        "pop r11",
        "pop rcx",
        "pop rsp",
        "sysretq",
        scratch = sym SCRATCH,
        stack_top = sym STACK_TOP,
        handler = sym HANDLER,
    )
}

/// Enters user mode at `entry` with stack `user_rsp`, and passes `a0`, `a1`, `a2`
/// in `rdi`, `rsi`, `rdx`. Does not return: the program leaves through a syscall
/// that its thread handles.
///
/// Interrupts are on in user mode (RFLAGS.IF is set), so ticks can preempt it.
/// The current thread's kernel stack must already be set with
/// [`set_kernel_stack`].
///
/// # Safety
///
/// `entry` must be code on pages that are user-accessible, and `user_rsp` must
/// be the top of a user-accessible, writable stack.
#[unsafe(naked)]
pub unsafe extern "sysv64" fn enter_user(
    entry: u64,
    user_rsp: u64,
    a0: u64,
    a1: u64,
    a2: u64,
) -> ! {
    naked_asm!(
        // The interrupt-return frame for ring 3: SS, RSP, RFLAGS, CS, RIP.
        "push {user_data}",
        "push rsi",
        "push {rflags}",
        "push {user_code}",
        "push rdi",
        "mov ax, {user_data}",
        "mov ds, ax",
        "mov es, ax",
        "mov rdi, rdx",
        "mov rsi, rcx",
        "mov rdx, r8",
        "iretq",
        user_data = const USER_DATA as u64,
        user_code = const USER_CODE as u64,
        rflags = const RFLAGS_IF | 0x2,
    )
}
