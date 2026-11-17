// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k HAL core: the hardware resources that the parsers produce and the
//! root server reserves and grants.
//!
//! A resource is a region of MMIO, an I/O port range or an interrupt line. The
//! root server checks that no two reservations collide before it grants any of
//! them. This crate holds the types and the collision rules only.

#![no_std]

#[cfg(test)]
extern crate std;

/// A physical MMIO region, `base .. base + len`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MmioRegion {
    /// First physical address.
    pub base: u64,
    /// Length in bytes. Zero-length regions are not valid and never overlap.
    pub len: u64,
}

impl MmioRegion {
    /// The region, or `None` if it is empty or wraps past the address space.
    #[must_use]
    pub const fn new(base: u64, len: u64) -> Option<Self> {
        if len == 0 || base.checked_add(len).is_none() {
            None
        } else {
            Some(Self { base, len })
        }
    }

    /// One past the last byte.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.base + self.len
    }

    /// True if the two regions share at least one byte.
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.base < other.end() && other.base < self.end()
    }

    /// True if `other` lies entirely inside this region.
    #[must_use]
    pub const fn contains(&self, other: &Self) -> bool {
        self.base <= other.base && other.end() <= self.end()
    }
}

/// How an interrupt line is triggered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Asserted for an instant.
    Edge,
    /// Held asserted until the device is serviced.
    Level,
}

/// The electrical sense of an interrupt line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Polarity {
    /// Asserted high.
    ActiveHigh,
    /// Asserted low.
    ActiveLow,
}

/// An interrupt line, named by its global system interrupt number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IrqLine {
    /// Global system interrupt number (the IO-APIC input index).
    pub gsi: u32,
    /// Edge or level.
    pub trigger: Trigger,
    /// Active high or low.
    pub polarity: Polarity,
}

/// A hardware resource a driver may be granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// A memory-mapped register or buffer range.
    Mmio(MmioRegion),
    /// A range of I/O ports, `base .. base + len`.
    Port {
        /// First port.
        base: u16,
        /// Number of ports.
        len: u16,
    },
    /// An interrupt line.
    Irq(IrqLine),
}

impl Resource {
    /// True if holding both resources at once would conflict: overlapping MMIO
    /// or ports, or the same interrupt line. The root server gives each line
    /// one owner, so any repeat of a GSI is a conflict, whatever its trigger.
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Mmio(a), Self::Mmio(b)) => a.overlaps(b),
            (Self::Port { base: a, len: al }, Self::Port { base: b, len: bl }) => {
                let a_end = *a as u32 + *al as u32;
                let b_end = *b as u32 + *bl as u32;
                *al != 0 && *bl != 0 && (*a as u32) < b_end && (*b as u32) < a_end
            }
            (Self::Irq(a), Self::Irq(b)) => a.gsi == b.gsi,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmio_regions_reject_empty_and_wrapping_ranges() {
        assert_eq!(MmioRegion::new(0x1000, 0), None);
        assert_eq!(MmioRegion::new(u64::MAX, 2), None);
        let r = MmioRegion::new(0x1000, 0x1000).unwrap();
        assert_eq!(r.end(), 0x2000);
    }

    #[test]
    fn mmio_overlap_and_containment() {
        let a = MmioRegion::new(0x1000, 0x1000).unwrap();
        let touching = MmioRegion::new(0x2000, 0x10).unwrap();
        let inside = MmioRegion::new(0x1800, 0x100).unwrap();
        let straddle = MmioRegion::new(0x1F00, 0x200).unwrap();
        assert!(!a.overlaps(&touching));
        assert!(a.overlaps(&straddle));
        assert!(a.contains(&inside));
        assert!(!a.contains(&straddle));
    }

    #[test]
    fn port_ranges_conflict_only_when_they_share_a_port() {
        let com1 = Resource::Port {
            base: 0x3F8,
            len: 8,
        };
        let com1_again = Resource::Port {
            base: 0x3FF,
            len: 1,
        };
        let com2 = Resource::Port {
            base: 0x2F8,
            len: 8,
        };
        assert!(com1.conflicts_with(&com1_again));
        assert!(!com1.conflicts_with(&com2));
        // A range that ends at 0xFFFF must not wrap into 0.
        let top = Resource::Port {
            base: 0xFFF8,
            len: 8,
        };
        assert!(!top.conflicts_with(&Resource::Port { base: 0, len: 1 }));
    }

    #[test]
    fn irq_lines_conflict_by_gsi() {
        let rtc = IrqLine {
            gsi: 8,
            trigger: Trigger::Edge,
            polarity: Polarity::ActiveHigh,
        };
        let other = IrqLine { gsi: 9, ..rtc };
        assert!(Resource::Irq(rtc).conflicts_with(&Resource::Irq(rtc)));
        assert!(!Resource::Irq(rtc).conflicts_with(&Resource::Irq(other)));
    }

    #[test]
    fn different_kinds_never_conflict() {
        let m = Resource::Mmio(MmioRegion::new(8, 8).unwrap());
        let p = Resource::Port { base: 8, len: 8 };
        assert!(!m.conflicts_with(&p));
    }
}
