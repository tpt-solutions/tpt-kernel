// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k HAL PCI(e): configuration-space enumeration and BAR decoding.
//!
//! The crate does not touch hardware itself. The caller implements
//! [`ConfigAccess`] over whatever reaches configuration space (the kernel uses
//! the ECAM window from the MCFG table), and this crate walks it.
//!
//! BAR sizing writes all ones to a BAR and reads the mask back. While that runs
//! the device's memory and I/O decoding are switched off in the command
//! register, as the PCI specification asks, and then restored.

#![no_std]

#[cfg(test)]
extern crate std;

/// Vendor ID that reads back when no function is present.
pub const VENDOR_NONE: u16 = 0xFFFF;

const REG_COMMAND: u16 = 0x04;
const REG_ID: u16 = 0x00;
const REG_CLASS: u16 = 0x08;
const REG_HEADER: u16 = 0x0C;
const REG_BAR0: u16 = 0x10;

/// Bus, device and function that name one PCI function in a segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bdf {
    /// Bus number, 0 to 255.
    pub bus: u8,
    /// Device number, 0 to 31.
    pub device: u8,
    /// Function number, 0 to 7.
    pub function: u8,
}

impl Bdf {
    /// The address of the function, or `None` if `device` or `function` is out
    /// of range.
    #[must_use]
    pub const fn new(bus: u8, device: u8, function: u8) -> Option<Self> {
        if device < 32 && function < 8 {
            Some(Self {
                bus,
                device,
                function,
            })
        } else {
            None
        }
    }

    /// Byte offset of this function's 4 KiB block inside an ECAM window that
    /// starts at the window's bus.
    #[must_use]
    pub const fn ecam_offset(self) -> u64 {
        ((self.bus as u64) << 20) | ((self.device as u64) << 15) | ((self.function as u64) << 12)
    }
}

/// Reads and writes 32-bit configuration registers.
pub trait ConfigAccess {
    /// Reads the dword at `offset`, which is 4-byte aligned and below 4096.
    fn read32(&mut self, bdf: Bdf, offset: u16) -> u32;
    /// Writes the dword at `offset`, which is 4-byte aligned and below 4096.
    fn write32(&mut self, bdf: Bdf, offset: u16, value: u32);
}

/// The identifying header fields of one present function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Function {
    /// Where the function sits.
    pub bdf: Bdf,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Device ID.
    pub device_id: u16,
    /// Base class code.
    pub class: u8,
    /// Subclass code.
    pub subclass: u8,
    /// Programming interface.
    pub prog_if: u8,
    /// Revision ID.
    pub revision: u8,
    /// Header layout: 0 for an endpoint, 1 for a PCI-to-PCI bridge, 2 for a
    /// CardBus bridge. The multi-function bit is masked off.
    pub header_type: u8,
    /// True when header bit 7 is set, so functions 1 to 7 may exist.
    pub multi_function: bool,
}

/// Reads the header of one function, or returns `None` if nothing answers.
pub fn read_function<A: ConfigAccess>(access: &mut A, bdf: Bdf) -> Option<Function> {
    let id = access.read32(bdf, REG_ID);
    let vendor_id = id as u16;
    if vendor_id == VENDOR_NONE {
        return None;
    }
    let class_word = access.read32(bdf, REG_CLASS);
    let header_word = access.read32(bdf, REG_HEADER);
    let header = (header_word >> 16) as u8;
    Some(Function {
        bdf,
        vendor_id,
        device_id: (id >> 16) as u16,
        revision: class_word as u8,
        prog_if: (class_word >> 8) as u8,
        subclass: (class_word >> 16) as u8,
        class: (class_word >> 24) as u8,
        header_type: header & 0x7F,
        multi_function: header & 0x80 != 0,
    })
}

/// Visits every present function on buses `start_bus..=end_bus`.
///
/// Function 0 of each device decides whether the device exists. Functions 1 to
/// 7 are read only when function 0 sets the multi-function bit. The walk does
/// not follow bridges, so the caller scans the buses it wants.
pub fn enumerate<A, F>(access: &mut A, start_bus: u8, end_bus: u8, mut visit: F)
where
    A: ConfigAccess,
    F: FnMut(Function),
{
    for bus in start_bus..=end_bus {
        for device in 0..32u8 {
            let Some(f0) = Bdf::new(bus, device, 0).and_then(|b| read_function(access, b)) else {
                continue;
            };
            let multi = f0.multi_function;
            visit(f0);
            if !multi {
                continue;
            }
            for function in 1..8u8 {
                if let Some(f) =
                    Bdf::new(bus, device, function).and_then(|b| read_function(access, b))
                {
                    visit(f);
                }
            }
        }
    }
}

/// Address space a BAR maps into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarKind {
    /// A 32-bit memory BAR.
    Memory32,
    /// A 64-bit memory BAR, which uses the next BAR slot for its upper half.
    Memory64,
    /// An I/O port BAR.
    Io,
}

/// A decoded base address register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    /// Register slot in the header, 0 to 5 for an endpoint.
    pub index: u8,
    /// Address space and width.
    pub kind: BarKind,
    /// Base address with the type bits masked off.
    pub base: u64,
    /// True for a memory BAR whose region has no read side effects.
    pub prefetchable: bool,
    /// Size of the region in bytes, from the sizing write. Zero if the BAR is
    /// not implemented.
    pub size: u64,
}

/// Decodes the raw register values of a BAR.
///
/// `low` is the BAR register itself. `high` is the next register, and is needed
/// only for a 64-bit memory BAR. Returns `None` for a reserved type or a 64-bit
/// BAR with no `high` value.
#[must_use]
pub fn decode_bar(low: u32, high: Option<u32>) -> Option<(BarKind, u64, bool)> {
    if low & 1 == 1 {
        return Some((BarKind::Io, (low & !0x3) as u64, false));
    }
    let prefetchable = low & 0x8 != 0;
    match (low >> 1) & 0x3 {
        0 => Some((BarKind::Memory32, (low & !0xF) as u64, prefetchable)),
        2 => {
            let high = high?;
            let base = ((high as u64) << 32) | (low & !0xF) as u64;
            Some((BarKind::Memory64, base, prefetchable))
        }
        _ => None,
    }
}

/// Computes a BAR's region size from the value it reads back after an
/// all-ones write.
///
/// `low` and `high` are the read-back values. The size is the lowest bit that
/// the BAR can hold, so a zero read-back means the BAR is not implemented.
#[must_use]
pub fn size_from_probe(kind: BarKind, low: u32, high: Option<u32>) -> u64 {
    match kind {
        BarKind::Io => {
            let masked = low & !0x3;
            if masked == 0 {
                0
            } else {
                (!masked).wrapping_add(1) as u64
            }
        }
        BarKind::Memory32 => {
            let masked = low & !0xF;
            if masked == 0 {
                0
            } else {
                (!masked).wrapping_add(1) as u64
            }
        }
        BarKind::Memory64 => {
            let value = ((high.unwrap_or(0) as u64) << 32) | (low & !0xF) as u64;
            if value == 0 {
                0
            } else {
                (!value).wrapping_add(1)
            }
        }
    }
}

/// Probes the BARs of a function and calls `visit` for each implemented one.
///
/// A header type 0 function has six BAR slots and a type 1 bridge has two.
/// Decoding is switched off in the command register while the sizing writes
/// run, and the registers are restored afterwards. A 64-bit BAR consumes the
/// slot after it.
pub fn probe_bars<A, F>(access: &mut A, bdf: Bdf, header_type: u8, mut visit: F)
where
    A: ConfigAccess,
    F: FnMut(Bar),
{
    let slots: u8 = match header_type {
        0 => 6,
        1 => 2,
        _ => return,
    };
    let command = access.read32(bdf, REG_COMMAND);
    // Keep the command bits, clear decoding, and write status as zero: status
    // bits are write-one-to-clear, so writing back what was read would clear them.
    access.write32(bdf, REG_COMMAND, command & 0xFFFC);

    let mut index = 0u8;
    while index < slots {
        let offset = REG_BAR0 + 4 * index as u16;
        let orig_low = access.read32(bdf, offset);
        let is_64 = orig_low & 0x1 == 0 && (orig_low >> 1) & 0x3 == 2;
        let orig_high = if is_64 {
            Some(access.read32(bdf, offset + 4))
        } else {
            None
        };
        let Some((kind, base, prefetchable)) = decode_bar(orig_low, orig_high) else {
            index += 1;
            continue;
        };

        access.write32(bdf, offset, 0xFFFF_FFFF);
        let probe_low = access.read32(bdf, offset);
        let probe_high = if is_64 {
            access.write32(bdf, offset + 4, 0xFFFF_FFFF);
            Some(access.read32(bdf, offset + 4))
        } else {
            None
        };

        access.write32(bdf, offset, orig_low);
        if let Some(h) = orig_high {
            access.write32(bdf, offset + 4, h);
        }

        let size = size_from_probe(kind, probe_low, probe_high);
        if size != 0 {
            visit(Bar {
                index,
                kind,
                base,
                prefetchable,
                size,
            });
        }
        index += if is_64 { 2 } else { 1 };
    }

    access.write32(bdf, REG_COMMAND, command & 0xFFFF);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::vec::Vec;

    /// One function's configuration dwords, with a per-dword write mask. A
    /// read-only register has a zero mask bit.
    struct Dev {
        regs: [u32; 64],
        writable: [u32; 64],
    }

    impl Dev {
        fn new(id: u32, class: u32, header: u32) -> Self {
            let mut regs = [0u32; 64];
            regs[0] = id;
            regs[2] = class;
            regs[3] = header;
            let mut writable = [0xFFFF_FFFF; 64];
            // Unimplemented BAR slots read as zero and ignore writes by default.
            writable[4..10].fill(0);
            Self { regs, writable }
        }
    }

    #[derive(Default)]
    struct Fake {
        devs: BTreeMap<Bdf, Dev>,
        writes: Vec<(Bdf, u16, u32)>,
    }

    impl ConfigAccess for Fake {
        fn read32(&mut self, bdf: Bdf, offset: u16) -> u32 {
            match self.devs.get(&bdf) {
                Some(d) => d.regs[(offset / 4) as usize],
                None => 0xFFFF_FFFF,
            }
        }

        fn write32(&mut self, bdf: Bdf, offset: u16, value: u32) {
            self.writes.push((bdf, offset, value));
            if let Some(d) = self.devs.get_mut(&bdf) {
                let i = (offset / 4) as usize;
                d.regs[i] = (value & d.writable[i]) | (d.regs[i] & !d.writable[i]);
            }
        }
    }

    fn bdf(b: u8, d: u8, f: u8) -> Bdf {
        Bdf::new(b, d, f).unwrap()
    }

    #[test]
    fn bdf_rejects_out_of_range_and_maps_ecam() {
        assert_eq!(Bdf::new(0, 32, 0), None);
        assert_eq!(Bdf::new(0, 0, 8), None);
        assert_eq!(
            bdf(1, 2, 3).ecam_offset(),
            (1 << 20) | (2 << 15) | (3 << 12)
        );
    }

    #[test]
    fn decode_memory_io_and_64_bit_bars() {
        assert_eq!(
            decode_bar(0xF000_0008, None),
            Some((BarKind::Memory32, 0xF000_0000, true))
        );
        assert_eq!(
            decode_bar(0x0000_C001, None),
            Some((BarKind::Io, 0xC000, false))
        );
        assert_eq!(
            decode_bar(0x0000_0004, Some(0x1)),
            Some((BarKind::Memory64, 0x1_0000_0000, false))
        );
        // A 64-bit BAR without its upper half cannot be decoded.
        assert_eq!(decode_bar(0x0000_0004, None), None);
        // Type 1 is reserved.
        assert_eq!(decode_bar(0x0000_0002, None), None);
    }

    #[test]
    fn size_from_probe_matches_region_size() {
        assert_eq!(
            size_from_probe(BarKind::Memory32, 0xFFF0_0000, None),
            0x10_0000
        );
        assert_eq!(size_from_probe(BarKind::Memory32, 0, None), 0);
        assert_eq!(
            size_from_probe(BarKind::Memory64, 0xC000_0004, Some(0xFFFF_FFFF)),
            0x4000_0000
        );
        assert_eq!(size_from_probe(BarKind::Io, 0xFFFF_FFC1, None), 0x40);
    }

    #[test]
    fn enumerate_finds_single_and_multi_function_devices() {
        let mut fake = Fake::default();
        fake.devs.insert(
            bdf(0, 0, 0),
            Dev::new(0x1237_8086, 0x0600_0002, 0x0000_0000),
        );
        // Device 3: multi-function, functions 0 and 2 present.
        fake.devs.insert(
            bdf(0, 3, 0),
            Dev::new(0x2922_8086, 0x0106_0100, 0x0080_0000),
        );
        fake.devs.insert(
            bdf(0, 3, 2),
            Dev::new(0x2923_8086, 0x0101_8000, 0x0000_0000),
        );
        let mut seen = Vec::new();
        enumerate(&mut fake, 0, 0, |f| {
            seen.push((f.bdf, f.vendor_id, f.class))
        });
        assert_eq!(
            seen,
            [
                (bdf(0, 0, 0), 0x8086, 0x06),
                (bdf(0, 3, 0), 0x8086, 0x01),
                (bdf(0, 3, 2), 0x8086, 0x01),
            ]
        );
    }

    #[test]
    fn function_fields_decode() {
        let mut fake = Fake::default();
        // revision 0x05, prog_if 0x02, subclass 0x08, class 0x01 (NVMe).
        fake.devs.insert(
            bdf(0, 1, 0),
            Dev::new(0xABCD_1B36, 0x0108_0205, 0x0080_0000),
        );
        let f = read_function(&mut fake, bdf(0, 1, 0)).unwrap();
        assert_eq!(f.device_id, 0xABCD);
        assert_eq!(f.vendor_id, 0x1B36);
        assert_eq!((f.class, f.subclass, f.prog_if, f.revision), (1, 8, 2, 5));
        assert!(f.multi_function);
        assert_eq!(f.header_type, 0);
    }

    #[test]
    fn probe_bars_sizes_32_and_64_bit_bars_and_restores_registers() {
        let mut fake = Fake::default();
        let b = bdf(0, 2, 0);
        let mut dev = Dev::new(0x1000_1AF4, 0x0200_0000, 0);
        // BAR0: 32-bit memory, 64 KiB, prefetchable at 0xF000_0000.
        dev.regs[4] = 0xF000_0008;
        dev.writable[4] = 0xFFFF_0000;
        // BAR1 and BAR2: a 64-bit memory BAR of 1 GiB at 0x1_0000_0000.
        dev.regs[5] = 0x0000_0004;
        dev.writable[5] = 0xC000_0000;
        dev.regs[6] = 0x0000_0001;
        dev.writable[6] = 0xFFFF_FFFF;
        // BAR3 is unimplemented.
        dev.regs[7] = 0;
        dev.writable[7] = 0;
        dev.regs[1] = 0x0000_0000; // command register, decoding on
        fake.devs.insert(b, dev);

        let mut bars = Vec::new();
        probe_bars(&mut fake, b, 0, |bar| bars.push(bar));
        assert_eq!(
            bars,
            [
                Bar {
                    index: 0,
                    kind: BarKind::Memory32,
                    base: 0xF000_0000,
                    prefetchable: true,
                    size: 0x1_0000,
                },
                Bar {
                    index: 1,
                    kind: BarKind::Memory64,
                    base: 0x1_0000_0000,
                    prefetchable: false,
                    size: 0x4000_0000,
                },
            ]
        );
        // The original values are back, and decoding is restored.
        assert_eq!(fake.devs[&b].regs[4], 0xF000_0008);
        assert_eq!(fake.devs[&b].regs[5], 0x0000_0004);
        assert_eq!(fake.devs[&b].regs[6], 0x0000_0001);
        assert_eq!(fake.devs[&b].regs[1] & 0x3, 0);
        // Decoding was switched off before the sizing writes.
        assert_eq!(fake.writes[0], (b, REG_COMMAND, 0));
    }

    #[test]
    fn probe_bars_on_bridge_uses_two_slots() {
        let mut fake = Fake::default();
        let b = bdf(0, 5, 0);
        let mut dev = Dev::new(0x0001_8086, 0x0604_0000, 0x0001_0000);
        dev.regs[4] = 0;
        dev.writable[4] = 0;
        dev.regs[5] = 0;
        dev.writable[5] = 0;
        fake.devs.insert(b, dev);
        let mut count = 0;
        probe_bars(&mut fake, b, 1, |_| count += 1);
        assert_eq!(count, 0);
    }
}
