// SPDX-License-Identifier: MIT OR Apache-2.0
//! The global descriptor table: flat 64-bit ring-0 code and data, flat ring-3
//! code and data, and the task state segment. The loader's GDT is replaced, so
//! the kernel owns its segment state from here on.

use core::arch::asm;
use core::ptr;

use crate::tss;

/// Selector of the kernel code segment.
pub const KERNEL_CODE: u16 = 0x08;
/// Selector of the kernel data segment.
pub const KERNEL_DATA: u16 = 0x10;
/// Selector of the user data segment, with RPL 3.
pub const USER_DATA: u16 = 0x18 | 3;
/// Selector of the user code segment, with RPL 3.
pub const USER_CODE: u16 = 0x20 | 3;
/// Selector of the task state segment. The TSS descriptor takes two slots.
pub const TSS_SELECTOR: u16 = 0x28;

/// Null, kernel code, kernel data, user data, user code, then two slots for the
/// TSS descriptor. Each flat segment has its accessed bit set, so the CPU does
/// not need to write the table when it loads a selector. The table is writable
/// because `ltr` sets the TSS busy bit.
static mut GDT: [u64; 7] = [
    0,
    // Kernel code: present, DPL 0, code, readable. L=1 (long mode), G=1.
    0x00AF_9B00_0000_FFFF,
    // Kernel data: present, DPL 0, data, writable. G=1, D/B=1.
    0x00CF_9300_0000_FFFF,
    // User data: present, DPL 3, data, writable.
    0x00CF_F300_0000_FFFF,
    // User code: present, DPL 3, code, readable. L=1, G=1.
    0x00AF_FB00_0000_FFFF,
    0,
    0,
];

/// The pointer operand of `lgdt` and `lidt`.
#[repr(C, packed)]
pub struct Pointer {
    limit: u16,
    base: u64,
}

/// The 64-bit available TSS descriptor for a TSS of `size` bytes at `base`.
fn tss_descriptor(base: u64, size: u64) -> [u64; 2] {
    let limit = size - 1;
    let low = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89u64 << 40)
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    [low, base >> 32]
}

/// Loads the GDT, reloads the segment registers, and loads the task register.
/// The TSS must already be initialized.
pub fn load() {
    let tss_base = tss::address();
    let tss_size = tss::size();
    // SAFETY: ring 0. `GDT` slots 5 and 6 are written before `lgdt` reads the
    // table, so the TSS descriptor is complete when `ltr` uses it. The `retfq`
    // and segment reloads use KERNEL_CODE, KERNEL_DATA and TSS_SELECTOR, all of
    // which name entries of this table. `pointer` lives on this stack frame across
    // the `lgdt`.
    unsafe {
        let table = ptr::addr_of_mut!(GDT).cast::<u64>();
        let [low, high] = tss_descriptor(tss_base, tss_size);
        table.add(5).write(low);
        table.add(6).write(high);
        let pointer = Pointer {
            limit: (7 * core::mem::size_of::<u64>() - 1) as u16,
            base: table as u64,
        };
        asm!(
            "lgdt [{ptr}]",
            "push {code}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {data:x}",
            "mov es, {data:x}",
            "mov ss, {data:x}",
            "ltr {tss:x}",
            ptr = in(reg) &pointer,
            code = in(reg) u64::from(KERNEL_CODE),
            data = in(reg) KERNEL_DATA,
            tss = in(reg) TSS_SELECTOR,
            tmp = lateout(reg) _,
            options(preserves_flags),
        );
    }
}
