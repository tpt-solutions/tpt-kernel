// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k kernel x86_64 primitives. Phase 1 covers port I/O, the COM1 serial
//! console, the QEMU debug-exit device used by the test harness, a flat GDT,
//! an IDT with exception handlers, the legacy PIC and PIT timer, and interrupt
//! state. APIC and paging come later.
//!
//! Every `unsafe` block states the invariant it relies on.

#![no_std]

use core::arch::asm;

pub mod apic;
pub mod gdt;
pub mod idt;
pub mod interrupts;
pub mod ioapic;
pub mod paging;
pub mod pic;
pub mod pit;
pub mod rtc;
pub mod syscall;
pub mod tss;

/// Writes one byte to an I/O port.
///
/// # Safety
///
/// Ring 0 only. `port` must be a device the kernel owns: a write to any other
/// port can change device state the kernel does not manage.
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    // SAFETY: the caller guarantees ring 0 and a kernel-owned port (see the
    // function contract). `out` touches no memory and keeps the flags.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value,
             options(nomem, nostack, preserves_flags));
    }
}

/// Reads one byte from an I/O port.
///
/// # Safety
///
/// Same contract as [`outb`]: ring 0 only, and a port the kernel owns.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: the caller guarantees ring 0 and a kernel-owned port. `in` reads
    // no memory and keeps the flags.
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port,
             options(nomem, nostack, preserves_flags));
    }
    value
}

/// Writes a 32-bit value to an I/O port.
///
/// # Safety
///
/// Same contract as [`outb`].
#[inline]
pub unsafe fn outl(port: u16, value: u32) {
    // SAFETY: as for `outb`.
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value,
             options(nomem, nostack, preserves_flags));
    }
}

/// The 16550 UART at COM1. Output only; polled, no interrupts.
pub mod serial {
    use super::{inb, outb};

    const COM1: u16 = 0x3F8;

    /// Programs COM1 for 115200 baud, 8 data bits, no parity, one stop bit.
    pub fn init() {
        // SAFETY: ring 0 (the kernel runs there), and COM1 is the kernel's
        // console. Each write programs one UART register at its documented
        // offset.
        unsafe {
            outb(COM1 + 1, 0x00); // disable interrupts
            outb(COM1 + 3, 0x80); // enable DLAB to set the divisor
            outb(COM1, 0x01); // divisor low byte: 1 (115200 baud)
            outb(COM1 + 1, 0x00); // divisor high byte
            outb(COM1 + 3, 0x03); // 8N1, DLAB off
            outb(COM1 + 2, 0xC7); // enable and clear FIFOs, 14-byte threshold
            outb(COM1 + 4, 0x0B); // DTR, RTS, OUT2
        }
    }

    /// Sends one byte, waiting until the transmit holding register is empty.
    pub fn write_byte(byte: u8) {
        // SAFETY: as for `init`. Bit 5 of the line status register is the
        // transmit-empty flag; polling it cannot change UART state.
        unsafe {
            while (inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            outb(COM1, byte);
        }
    }

    /// Sends a string, translating `\n` to `\r\n`.
    pub fn write_str(text: &str) {
        for byte in text.bytes() {
            if byte == b'\n' {
                write_byte(b'\r');
            }
            write_byte(byte);
        }
    }

    /// `core::fmt::Write` over COM1, so `write!` works in the kernel.
    pub struct Writer;

    impl core::fmt::Write for Writer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            write_str(text);
            Ok(())
        }
    }
}

/// The QEMU `isa-debug-exit` device, configured with `iobase=0xf4`.
pub mod qemu {
    use super::outl;

    /// Port of the debug-exit device.
    pub const EXIT_PORT: u16 = 0xF4;

    /// Exit code written to the device for a passing run. QEMU reports
    /// `(code << 1) | 1`, so the host sees 33.
    pub const EXIT_SUCCESS: u32 = 0x10;

    /// Exit code written to the device for a failing run (host sees 35).
    pub const EXIT_FAILURE: u32 = 0x11;

    /// Stops the virtual machine with `code`. Without the device this halts
    /// the CPU forever.
    pub fn exit(code: u32) -> ! {
        // SAFETY: ring 0. Writing the debug-exit port has no effect unless
        // QEMU has the device configured, and it never touches memory.
        unsafe { outl(EXIT_PORT, code) };
        halt_forever()
    }

    /// Halts the CPU with interrupts disabled. Never returns.
    pub fn halt_forever() -> ! {
        loop {
            // SAFETY: `cli` and `hlt` only change the CPU's interrupt and halt
            // state. Phase 1 does not rely on interrupts.
            unsafe { core::arch::asm!("cli", "hlt", options(nomem, nostack)) };
        }
    }
}
