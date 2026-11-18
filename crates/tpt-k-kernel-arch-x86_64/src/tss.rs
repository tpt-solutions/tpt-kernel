// SPDX-License-Identifier: MIT OR Apache-2.0
//! The task state segment. The CPU reads `rsp0` from it when an interrupt or a
//! syscall arrives from ring 3, so the kernel stack is set here.

use core::ptr::addr_of_mut;

/// The 64-bit TSS layout (Intel SDM vol. 3, figure 8-11).
#[repr(C, packed)]
pub struct Tss {
    reserved0: u32,
    /// Stack pointer for ring 0.
    pub rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    /// Offset of the I/O permission bitmap. Past the end of the segment means
    /// no bitmap, so every port access from ring 3 faults.
    iopb: u16,
}

static mut TSS: Tss = Tss {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iopb: core::mem::size_of::<Tss>() as u16,
};

/// Address of the TSS, for its GDT descriptor.
#[must_use]
pub fn address() -> u64 {
    addr_of_mut!(TSS) as u64
}

/// Size of the TSS in bytes, for its GDT descriptor.
#[must_use]
pub const fn size() -> u64 {
    core::mem::size_of::<Tss>() as u64
}

/// Sets the stack that ring-3 entries switch to. `top` is the first address past
/// the stack, and it must be 16-byte aligned.
pub fn set_kernel_stack(top: u64) {
    // SAFETY: the kernel runs on one CPU, and the TSS is read by that CPU only on
    // a ring-3 to ring-0 transition. That cannot overlap this kernel-mode store,
    // and a single aligned u64 write is not torn. `rsp0` is a packed field, but
    // assigning to it writes the value in place without creating a reference.
    unsafe {
        (*addr_of_mut!(TSS)).rsp0 = top;
    }
}
