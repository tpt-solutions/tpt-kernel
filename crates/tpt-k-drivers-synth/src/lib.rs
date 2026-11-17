// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k synth engine: matching PCI-style hardware IDs against known layouts.
//!
//! A [`Layout`] is a rule for a family of devices. It matches on an exact
//! vendor and device pair, or on class codes, or on any mix of the fields
//! that the rule sets. [`match_best`] picks the rule that describes a device
//! most closely. Generating micro-drivers is not here yet.

#![no_std]

#[cfg(test)]
extern crate std;

/// The identifying fields of one PCI-style function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareId {
    /// Vendor ID.
    pub vendor: u16,
    /// Device ID.
    pub device: u16,
    /// Base class code.
    pub class: u8,
    /// Subclass code.
    pub subclass: u8,
    /// Programming interface.
    pub prog_if: u8,
}

/// A rule that matches a family of devices. A field set to `None` matches
/// anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Name of the layout, for logs and for choosing a driver.
    pub name: &'static str,
    /// Required vendor ID, or `None`.
    pub vendor: Option<u16>,
    /// Required device ID, or `None`.
    pub device: Option<u16>,
    /// Required base class, or `None`.
    pub class: Option<u8>,
    /// Required subclass, or `None`.
    pub subclass: Option<u8>,
    /// Required programming interface, or `None`.
    pub prog_if: Option<u8>,
    /// Tie-break between rules that are equally specific. Higher wins.
    pub priority: u8,
}

impl Layout {
    /// True if every field this rule sets equals the device's field.
    #[must_use]
    pub fn matches(&self, id: &HardwareId) -> bool {
        self.vendor.is_none_or(|v| v == id.vendor)
            && self.device.is_none_or(|d| d == id.device)
            && self.class.is_none_or(|c| c == id.class)
            && self.subclass.is_none_or(|s| s == id.subclass)
            && self.prog_if.is_none_or(|p| p == id.prog_if)
    }

    /// Ordering key for a matching rule: an exact vendor and device pair
    /// outranks any class rule, then more set fields outrank fewer, then
    /// priority decides.
    fn specificity(&self) -> (bool, u8, u8) {
        let exact_id = self.vendor.is_some() && self.device.is_some();
        let fields = [
            self.vendor.is_some(),
            self.device.is_some(),
            self.class.is_some(),
            self.subclass.is_some(),
            self.prog_if.is_some(),
        ]
        .iter()
        .filter(|set| **set)
        .count() as u8;
        (exact_id, fields, self.priority)
    }
}

/// Returns the most specific layout that matches `id`.
///
/// Order of preference: a rule with both vendor and device set beats any rule
/// that does not; then the rule that sets more fields; then the higher
/// priority. A complete tie goes to the rule listed first. Returns `None` if
/// nothing matches.
#[must_use]
pub fn match_best<'a>(layouts: &'a [Layout], id: &HardwareId) -> Option<&'a Layout> {
    let mut best: Option<&Layout> = None;
    for candidate in layouts.iter().filter(|l| l.matches(id)) {
        best = match best {
            Some(current) if current.specificity() >= candidate.specificity() => Some(current),
            _ => Some(candidate),
        };
    }
    best
}

/// Built-in layouts for the devices the first drivers target.
pub const BUILTIN: &[Layout] = &[
    Layout {
        name: "virtio-blk (legacy)",
        vendor: Some(0x1AF4),
        device: Some(0x1001),
        class: None,
        subclass: None,
        prog_if: None,
        priority: 10,
    },
    Layout {
        name: "virtio-blk",
        vendor: Some(0x1AF4),
        device: Some(0x1042),
        class: None,
        subclass: None,
        prog_if: None,
        priority: 10,
    },
    Layout {
        name: "nvme",
        vendor: None,
        device: None,
        class: Some(0x01),
        subclass: Some(0x08),
        prog_if: Some(0x02),
        priority: 10,
    },
    Layout {
        name: "xhci",
        vendor: None,
        device: None,
        class: Some(0x0C),
        subclass: Some(0x03),
        prog_if: Some(0x30),
        priority: 10,
    },
    Layout {
        name: "usb-host (HID-capable)",
        vendor: None,
        device: None,
        class: Some(0x0C),
        subclass: Some(0x03),
        prog_if: None,
        priority: 1,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn id(vendor: u16, device: u16, class: u8, subclass: u8, prog_if: u8) -> HardwareId {
        HardwareId {
            vendor,
            device,
            class,
            subclass,
            prog_if,
        }
    }

    const fn rule(
        name: &'static str,
        vendor: Option<u16>,
        device: Option<u16>,
        class: Option<u8>,
        subclass: Option<u8>,
        prog_if: Option<u8>,
        priority: u8,
    ) -> Layout {
        Layout {
            name,
            vendor,
            device,
            class,
            subclass,
            prog_if,
            priority,
        }
    }

    #[test]
    fn exact_id_beats_a_class_rule_even_with_lower_priority() {
        let table = [
            rule("class", None, None, Some(0x0C), Some(0x03), None, 200),
            rule("exact", Some(0x1234), Some(0x5678), None, None, None, 1),
        ];
        let dev = id(0x1234, 0x5678, 0x0C, 0x03, 0x30);
        assert_eq!(match_best(&table, &dev).unwrap().name, "exact");
    }

    #[test]
    fn a_class_rule_with_more_fields_beats_one_with_fewer() {
        let table = [
            rule("class", None, None, Some(0x0C), None, None, 200),
            rule(
                "class+prog",
                None,
                None,
                Some(0x0C),
                Some(0x03),
                Some(0x30),
                1,
            ),
        ];
        let dev = id(0, 0, 0x0C, 0x03, 0x30);
        assert_eq!(match_best(&table, &dev).unwrap().name, "class+prog");
    }

    #[test]
    fn priority_breaks_ties_between_equally_specific_rules() {
        let table = [
            rule("low", None, None, Some(0x01), None, None, 1),
            rule("high", None, None, Some(0x01), None, None, 9),
        ];
        let dev = id(0, 0, 0x01, 0, 0);
        assert_eq!(match_best(&table, &dev).unwrap().name, "high");
    }

    #[test]
    fn first_listed_wins_a_complete_tie() {
        let table = [
            rule("first", None, None, Some(0x02), None, None, 5),
            rule("second", None, None, Some(0x02), None, None, 5),
        ];
        let dev = id(0, 0, 0x02, 0, 0);
        assert_eq!(match_best(&table, &dev).unwrap().name, "first");
    }

    #[test]
    fn a_rule_that_does_not_match_is_skipped() {
        let table = [
            rule("exact", Some(0x1234), Some(0x5678), None, None, None, 255),
            rule("class", None, None, Some(0x01), None, None, 0),
        ];
        // Same vendor but another device: the exact rule must not match.
        let dev = id(0x1234, 0x9999, 0x01, 0, 0);
        assert_eq!(match_best(&table, &dev).unwrap().name, "class");
        // Nothing matches at all.
        assert_eq!(match_best(&table, &id(0, 0, 0x07, 0, 0)), None);
        assert_eq!(match_best(&[], &dev), None);
    }

    #[test]
    fn builtin_matches_virtio_block_devices_by_id() {
        let legacy = id(0x1AF4, 0x1001, 0x01, 0x80, 0);
        assert_eq!(
            match_best(BUILTIN, &legacy).unwrap().name,
            "virtio-blk (legacy)"
        );
        let modern = id(0x1AF4, 0x1042, 0x01, 0x80, 0);
        assert_eq!(match_best(BUILTIN, &modern).unwrap().name, "virtio-blk");
    }

    #[test]
    fn builtin_nvme_needs_all_three_class_codes() {
        assert_eq!(
            match_best(BUILTIN, &id(0x8086, 0x0A54, 0x01, 0x08, 0x02))
                .unwrap()
                .name,
            "nvme"
        );
        // NVMe class but a different interface is not NVMe.
        assert_eq!(
            match_best(BUILTIN, &id(0x8086, 0x0A54, 0x01, 0x08, 0x01)),
            None
        );
    }

    #[test]
    fn builtin_xhci_outranks_the_generic_usb_host_rule() {
        let xhci = id(0x8086, 0x1E31, 0x0C, 0x03, 0x30);
        assert_eq!(match_best(BUILTIN, &xhci).unwrap().name, "xhci");
        // An EHCI or UHCI controller (other prog_if) falls back to the generic rule.
        let ehci = id(0x8086, 0x1E2D, 0x0C, 0x03, 0x20);
        assert_eq!(
            match_best(BUILTIN, &ehci).unwrap().name,
            "usb-host (HID-capable)"
        );
    }

    #[test]
    fn matches_checks_only_the_fields_a_rule_sets() {
        let r = rule("class", None, None, Some(0x0C), None, None, 0);
        assert!(r.matches(&id(0xFFFF, 0xFFFF, 0x0C, 0x99, 0x99)));
        assert!(!r.matches(&id(0, 0, 0x0D, 0, 0)));
    }
}
