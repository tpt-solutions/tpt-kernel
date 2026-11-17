// SPDX-License-Identifier: MIT OR Apache-2.0
//! The CMOS real-time clock's periodic interrupt. It is a device interrupt that
//! the test uses to exercise the IO-APIC route end to end.
//!
//! The clock's registers are reached through two ports: the index port selects a
//! register and the data port reads or writes it. Interrupt flags are cleared by
//! reading register C, and the interrupt keeps firing only while that happens.

use crate::{inb, outb};

const INDEX_PORT: u16 = 0x70;
const DATA_PORT: u16 = 0x71;

const REG_A: u8 = 0x0A;
const REG_B: u8 = 0x0B;
const REG_C: u8 = 0x0C;

/// Register B bit 6: periodic interrupt enable.
const PERIODIC_ENABLE: u8 = 1 << 6;

/// The rate select for a 2 Hz periodic interrupt (register A bits 0 to 3).
pub const RATE_2_HZ: u8 = 15;

fn read(register: u8) -> u8 {
    // SAFETY: ring 0. The index and data ports belong to the CMOS clock, which
    // the kernel owns. Selecting a register only changes what the data port
    // reads, and NMI stays enabled (bit 7 clear).
    unsafe {
        outb(INDEX_PORT, register);
        inb(DATA_PORT)
    }
}

fn write(register: u8, value: u8) {
    // SAFETY: as for `read`.
    unsafe {
        outb(INDEX_PORT, register);
        outb(DATA_PORT, value);
    }
}

/// Starts the periodic interrupt at `rate` (see [`RATE_2_HZ`]). The interrupt
/// comes out on the RTC line, which is IRQ 8.
pub fn start_periodic(rate: u8) {
    write(REG_A, (read(REG_A) & 0xF0) | (rate & 0x0F));
    write(REG_B, read(REG_B) | PERIODIC_ENABLE);
    acknowledge();
}

/// Stops the periodic interrupt.
pub fn stop() {
    write(REG_B, read(REG_B) & !PERIODIC_ENABLE);
    acknowledge();
}

/// Clears the pending interrupt flags. The handler must call this, or the clock
/// stops interrupting.
pub fn acknowledge() {
    let _ = read(REG_C);
}
