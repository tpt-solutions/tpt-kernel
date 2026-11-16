// SPDX-License-Identifier: MIT OR Apache-2.0
//! The 8259 programmable interrupt controllers. They are remapped above the
//! CPU exception vectors, and all lines are masked except the timer (IRQ 0).

use crate::outb;

const MASTER_COMMAND: u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_COMMAND: u16 = 0xA0;
const SLAVE_DATA: u16 = 0xA1;

/// First vector of the master controller's lines (IRQ 0 to 7).
pub const MASTER_BASE: u8 = 32;
/// First vector of the slave controller's lines (IRQ 8 to 15).
pub const SLAVE_BASE: u8 = 40;

/// The end-of-interrupt command.
const EOI: u8 = 0x20;

/// Remaps both controllers and masks every line except IRQ 0.
pub fn init() {
    // SAFETY: ring 0. These are the PIC ports, which the kernel owns. The
    // sequence is the documented initialization order: ICW1 starts it, ICW2
    // sets the vector bases, ICW3 wires the cascade on IRQ 2, and ICW4 selects
    // 8086 mode. The last writes set the interrupt masks.
    unsafe {
        outb(MASTER_COMMAND, 0x11);
        outb(SLAVE_COMMAND, 0x11);
        outb(MASTER_DATA, MASTER_BASE);
        outb(SLAVE_DATA, SLAVE_BASE);
        outb(MASTER_DATA, 0x04);
        outb(SLAVE_DATA, 0x02);
        outb(MASTER_DATA, 0x01);
        outb(SLAVE_DATA, 0x01);
        outb(MASTER_DATA, 0xFE);
        outb(SLAVE_DATA, 0xFF);
    }
}

/// Masks every line of both controllers.
pub fn mask_all() {
    // SAFETY: ring 0. Writing 0xFF to the data ports masks all lines and changes no
    // other state.
    unsafe {
        outb(MASTER_DATA, 0xFF);
        outb(SLAVE_DATA, 0xFF);
    }
}

/// Acknowledges interrupt `line` (0 to 15). Lines 8 to 15 go through the slave
/// controller, which needs its own acknowledgement.
pub fn eoi(line: u8) {
    // SAFETY: ring 0, and the command port only accepts the EOI command.
    unsafe {
        if line >= 8 {
            outb(SLAVE_COMMAND, EOI);
        }
        outb(MASTER_COMMAND, EOI);
    }
}
