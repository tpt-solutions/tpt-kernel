// SPDX-License-Identifier: MIT OR Apache-2.0
//! The IO-APIC: routes device interrupt lines (global system interrupts, GSIs)
//! to interrupt vectors on a local APIC.
//!
//! The registers are reached indirectly: write the register index to `IOREGSEL`,
//! then read or write `IOWIN`. The kernel maps the page uncached (see
//! `paging::NO_CACHE`). Nothing is routed until [`route`] is called, so the PIC
//! lines stay masked until a driver asks for them.

use core::ptr::{read_volatile, write_volatile};

/// Physical address of the IO-APIC registers. The default location, which the
/// firmware keeps unless it moves the device.
pub const IOAPIC_BASE: u64 = 0xFEC0_0000;

const REG_SELECT: u64 = 0x00;
const REG_WINDOW: u64 = 0x10;

/// Index of the version register.
const REG_VERSION: u32 = 0x01;
/// Index of the first redirection entry. Each entry is two registers, low and
/// high.
const REG_REDIRECTION: u32 = 0x10;

fn read(index: u32) -> u32 {
    // SAFETY: the page at IOAPIC_BASE is mapped uncached by the kernel. The
    // select and window registers are device registers at fixed offsets.
    unsafe {
        write_volatile((IOAPIC_BASE + REG_SELECT) as *mut u32, index);
        read_volatile((IOAPIC_BASE + REG_WINDOW) as *const u32)
    }
}

fn write(index: u32, value: u32) {
    // SAFETY: as for `read`.
    unsafe {
        write_volatile((IOAPIC_BASE + REG_SELECT) as *mut u32, index);
        write_volatile((IOAPIC_BASE + REG_WINDOW) as *mut u32, value);
    }
}

/// The IO-APIC version, from its version register.
#[must_use]
pub fn version() -> u8 {
    (read(REG_VERSION) & 0xFF) as u8
}

/// Routes global interrupt `gsi` to `vector` on the local APIC `apic_id`, as an
/// edge-triggered, active-high, fixed-delivery interrupt, and unmasks it.
///
/// The high half, which names the destination, is written before the low half,
/// which unmasks the entry, so the line never fires towards a wrong CPU.
pub fn route(gsi: u32, vector: u8, apic_id: u8) {
    let entry = REG_REDIRECTION + 2 * gsi;
    write(entry + 1, u32::from(apic_id) << 24);
    write(entry, u32::from(vector));
}
