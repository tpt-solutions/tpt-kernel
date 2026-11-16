// SPDX-License-Identifier: MIT OR Apache-2.0
//! The 8254 programmable interval timer. Channel 0 drives IRQ 0.

use crate::{inb, outb};

const CHANNEL0: u16 = 0x40;
const COMMAND: u16 = 0x43;
/// The timer's input clock in hertz.
const BASE_HZ: u32 = 1_193_182;

/// Starts channel 0 as a periodic interrupt source at about `hz` hertz.
///
/// Returns `false` if `hz` is zero or too low for the 16-bit divisor.
pub fn set_periodic(hz: u32) -> bool {
    let Some(divisor) = BASE_HZ.checked_div(hz) else {
        return false;
    };
    let Ok(divisor) = u16::try_from(divisor) else {
        return false;
    };
    // SAFETY: ring 0. The command byte selects channel 0, a low-then-high
    // access, mode 3 (square wave) and binary counting. The two data bytes are
    // the divisor, low byte first, as the access mode requires.
    unsafe {
        outb(COMMAND, 0x36);
        outb(CHANNEL0, divisor as u8);
        outb(CHANNEL0, (divisor >> 8) as u8);
    }
    true
}

/// Busy-waits for 10 ms, timed by channel 2 in one-shot mode. The gate and speaker
/// bits of port 0x61 are restored afterwards.
pub fn wait_10ms() {
    const PORT_B: u16 = 0x61;
    const CHANNEL2: u16 = 0x42;
    // SAFETY: ring 0. Port 0x61 bit 0 gates channel 2, bit 1 drives the speaker,
    // and bit 5 reports channel 2's output. The channel 2 command and count are
    // the documented one-shot sequence.
    unsafe {
        let saved = inb(PORT_B);
        outb(PORT_B, (saved & !0x02) | 0x01);
        outb(COMMAND, 0xB0);
        let count = BASE_HZ / 100;
        outb(CHANNEL2, count as u8);
        outb(CHANNEL2, (count >> 8) as u8);
        while inb(PORT_B) & 0x20 == 0 {
            core::hint::spin_loop();
        }
        outb(PORT_B, saved);
    }
}
