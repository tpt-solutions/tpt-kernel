// SPDX-License-Identifier: MIT OR Apache-2.0
//! Interrupt enable state. A kernel critical section disables interrupts and
//! restores whatever state it found, so sections nest.

use core::arch::asm;

/// The interrupt flag in RFLAGS.
const IF: u64 = 1 << 9;

/// Enables interrupts.
pub fn enable() {
    // SAFETY: ring 0. `sti` only sets the interrupt flag. The IDT is loaded
    // before the kernel calls this.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// Disables interrupts.
pub fn disable() {
    // SAFETY: ring 0. `cli` only clears the interrupt flag.
    unsafe { asm!("cli", options(nomem, nostack)) };
}

/// True if interrupts are enabled.
#[must_use]
pub fn enabled() -> bool {
    let flags: u64;
    // SAFETY: ring 0. `pushfq` and `pop` move RFLAGS through the stack, and
    // neither changes the flags.
    unsafe {
        asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags));
    }
    flags & IF != 0
}

/// Runs `f` with interrupts disabled, then restores the previous state.
///
/// `f` may switch threads. Each thread restores its own state when it leaves its
/// section.
pub fn without<T>(f: impl FnOnce() -> T) -> T {
    let was_enabled = enabled();
    disable();
    let result = f();
    if was_enabled {
        enable();
    }
    result
}
