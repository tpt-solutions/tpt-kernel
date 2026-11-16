// SPDX-License-Identifier: MIT OR Apache-2.0
//! The x86_64 context switch. A switched-out thread's callee-saved registers
//! sit on its own stack, and the stack pointer is the only state saved in the
//! thread record.

/// Words in a saved context frame: six callee-saved registers, the return
/// address the switch returns to, and one padding word, so that the entry
/// function starts with a stack that is 16-byte aligned plus 8, as the System V
/// ABI requires at function entry.
pub const FRAME_WORDS: usize = 8;

/// Index in the frame of the return address that a new thread enters through.
pub const ENTRY_WORD: usize = 6;

/// Switches from the running thread to another one.
///
/// Saves the callee-saved registers on the current stack, stores the stack
/// pointer at `save`, loads `next_rsp`, and restores that thread's registers.
/// The function returns on the new thread's stack.
///
/// The x87 and SSE control state is not saved. The kernel uses neither, so it is
/// not switched.
///
/// # Safety
///
/// `save` must point to a writable `u64` that outlives the switch. `next_rsp`
/// must be a stack pointer produced by an earlier switch or by
/// [`initial_frame`], and its stack must still be live.
#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
pub unsafe extern "sysv64" fn switch(save: *mut u64, next_rsp: u64) {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    )
}

/// Why a stack cannot hold a new thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackError {
    /// The stack has fewer than [`FRAME_WORDS`] words.
    TooSmall,
    /// The end of the stack (its base address plus its length) is not 16-byte
    /// aligned.
    Misaligned,
}

/// Writes the initial context frame at the top of `stack`, so that the first
/// switch to the new thread jumps to `entry`. Returns the stack pointer to store
/// in the thread record.
///
/// The frame occupies the last [`FRAME_WORDS`] words. Its layout from the stack
/// pointer upward is six zeroed registers, then `entry`, then one zeroed word.
/// The end of the stack must be 16-byte aligned, so that the entry function
/// starts with `rsp % 16 == 8`, as the System V ABI requires.
///
/// # Errors
///
/// Returns [`StackError::TooSmall`] if the stack cannot hold the frame, and
/// [`StackError::Misaligned`] if its end is not 16-byte aligned.
pub fn initial_stack(stack: &mut [u64], entry: u64) -> Result<u64, StackError> {
    if stack.len() < FRAME_WORDS {
        return Err(StackError::TooSmall);
    }
    let base = stack.as_ptr() as u64;
    let top = base + (stack.len() as u64) * 8;
    if !top.is_multiple_of(16) {
        return Err(StackError::Misaligned);
    }
    let start = stack.len() - FRAME_WORDS;
    let frame = &mut stack[start..];
    frame.fill(0);
    frame[ENTRY_WORD] = entry;
    Ok(base + (start as u64) * 8)
}
