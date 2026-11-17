// SPDX-License-Identifier: MIT OR Apache-2.0
//! The local APIC: the timer that drives preemption, and end-of-interrupt.
//!
//! The registers are memory-mapped at [`APIC_BASE`]. The kernel maps that page
//! uncached (see `paging::NO_CACHE`), so each register access reaches the device.
//! The timer is calibrated against the PIT, so its rate does not depend on the
//! clock of the machine it runs on.

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

use crate::{pic, pit};

/// Physical address of the local APIC registers. The default location; the
/// kernel does not move it.
pub const APIC_BASE: u64 = 0xFEE0_0000;

const MSR_APIC_BASE: u32 = 0x1B;
/// The APIC global enable bit in `IA32_APIC_BASE`.
const APIC_GLOBAL_ENABLE: u64 = 1 << 11;

const REG_ID: u64 = 0x20;
const REG_EOI: u64 = 0xB0;
const REG_SPURIOUS: u64 = 0xF0;
const REG_LVT_TIMER: u64 = 0x320;
const REG_TIMER_INITIAL: u64 = 0x380;
const REG_TIMER_CURRENT: u64 = 0x390;
const REG_TIMER_DIVIDE: u64 = 0x3E0;

/// Spurious-interrupt register: the APIC is software-enabled.
const SPURIOUS_ENABLE: u32 = 1 << 8;
/// The vector used for spurious interrupts. It has no handler.
const SPURIOUS_VECTOR: u32 = 0xFF;
/// LVT bit that masks an interrupt source.
const LVT_MASKED: u32 = 1 << 16;
/// LVT timer mode: periodic.
const LVT_PERIODIC: u32 = 1 << 17;
/// Divide the timer clock by 16.
const DIVIDE_BY_16: u32 = 0b0011;

fn read(offset: u64) -> u32 {
    // SAFETY: the APIC page is mapped (uncached) by the kernel's address space,
    // and the offsets are the documented 32-bit register offsets inside it.
    unsafe { read_volatile((APIC_BASE + offset) as *const u32) }
}

fn write(offset: u64, value: u32) {
    // SAFETY: as for `read`. The register write takes effect immediately.
    unsafe { write_volatile((APIC_BASE + offset) as *mut u32, value) };
}

fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    // SAFETY: `IA32_APIC_BASE` exists on every x86_64 CPU with an APIC.
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high,
             options(nomem, nostack, preserves_flags));
    }
    (u64::from(high) << 32) | u64::from(low)
}

fn write_msr(msr: u32, value: u64) {
    // SAFETY: called with the value read from the same register plus the enable
    // bit, so the other fields keep their firmware values.
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32,
             options(nomem, nostack, preserves_flags));
    }
}

/// Enables the local APIC, sets its spurious vector, and masks its timer. Also
/// masks every PIC line, since the APIC replaces the PIC for the timer.
pub fn enable() {
    write_msr(MSR_APIC_BASE, read_msr(MSR_APIC_BASE) | APIC_GLOBAL_ENABLE);
    write(REG_SPURIOUS, SPURIOUS_ENABLE | SPURIOUS_VECTOR);
    write(REG_LVT_TIMER, LVT_MASKED);
    pic::mask_all();
}

/// The ID of the running CPU's local APIC, as the IO-APIC names it in a route.
#[must_use]
pub fn id() -> u8 {
    (read(REG_ID) >> 24) as u8
}

/// Acknowledges the interrupt being handled.
pub fn eoi() {
    write(REG_EOI, 0);
}

/// Measures how many APIC timer ticks pass in 10 ms, using the PIT as the
/// reference. The timer is left stopped and masked.
#[must_use]
pub fn calibrate_10ms() -> u32 {
    write(REG_TIMER_DIVIDE, DIVIDE_BY_16);
    write(REG_LVT_TIMER, LVT_MASKED);
    write(REG_TIMER_INITIAL, u32::MAX);
    pit::wait_10ms();
    u32::MAX - read(REG_TIMER_CURRENT)
}

/// Starts the timer in periodic mode on `vector`, with `initial` ticks per
/// period.
pub fn start_periodic(vector: u8, initial: u32) {
    write(REG_TIMER_DIVIDE, DIVIDE_BY_16);
    write(REG_LVT_TIMER, u32::from(vector) | LVT_PERIODIC);
    write(REG_TIMER_INITIAL, initial);
}
