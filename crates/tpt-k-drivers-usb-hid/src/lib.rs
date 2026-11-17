// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k USB HID driver core: report descriptor parsing and the boot-protocol
//! keyboard and mouse reports.
//!
//! The crate is `no_std` and allocates nothing. It decodes bytes that the
//! driver has already received; moving them over USB is the host controller
//! driver's job. A keyboard that answers with the boot protocol needs no
//! descriptor: [`KeyboardReport::parse`] reads its 8-byte report directly.
//! A device that uses the report protocol is described by [`ReportMap`].

#![no_std]

#[cfg(test)]
extern crate std;

pub mod boot;
pub mod descriptor;

pub use boot::{
    new_presses, usage_to_char, KeyList, KeyboardReport, Modifiers, MouseReport,
    KEY_ERROR_ROLLOVER, KEY_SLOTS,
};
pub use descriptor::{
    bits_at, Field, ReportMap, FLAG_CONSTANT, FLAG_RELATIVE, FLAG_VARIABLE, MAX_FIELDS, MAX_REPORTS,
};

/// Usage page for the Generic Desktop controls (mouse, joystick, keyboard).
pub const PAGE_GENERIC_DESKTOP: u16 = 0x01;
/// Usage page for keyboard and keypad keys.
pub const PAGE_KEYBOARD: u16 = 0x07;
/// Usage page for LEDs.
pub const PAGE_LEDS: u16 = 0x08;
/// Usage page for buttons.
pub const PAGE_BUTTON: u16 = 0x09;

/// Why a descriptor or report could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidError {
    /// The input ends inside an item, or a report is shorter than its format.
    Truncated,
    /// A reserved item type or an item this parser does not know.
    BadItem,
    /// More input fields than [`MAX_FIELDS`].
    TooManyFields,
    /// More report IDs than [`MAX_REPORTS`].
    TooManyReports,
    /// A report's bits do not fit the arithmetic used to lay it out.
    ReportTooLarge,
    /// A `Collection` or `End Collection` does not balance.
    Unbalanced,
    /// More `Push` items than the state stack holds.
    StackOverflow,
    /// A `Pop` with nothing pushed.
    StackUnderflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// The boot keyboard report descriptor (HID 1.11, appendix B.1): 63 bytes.
    const BOOT_KEYBOARD: [u8; 63] = [
        0x05, 0x01, // Usage Page (Generic Desktop)
        0x09, 0x06, // Usage (Keyboard)
        0xA1, 0x01, // Collection (Application)
        0x05, 0x07, //   Usage Page (Key Codes)
        0x19, 0xE0, //   Usage Minimum (224)
        0x29, 0xE7, //   Usage Maximum (231)
        0x15, 0x00, //   Logical Minimum (0)
        0x25, 0x01, //   Logical Maximum (1)
        0x75, 0x01, //   Report Size (1)
        0x95, 0x08, //   Report Count (8)
        0x81, 0x02, //   Input (Data, Variable, Absolute): modifiers
        0x95, 0x01, //   Report Count (1)
        0x75, 0x08, //   Report Size (8)
        0x81, 0x01, //   Input (Constant): reserved byte
        0x95, 0x05, //   Report Count (5)
        0x75, 0x01, //   Report Size (1)
        0x05, 0x08, //   Usage Page (LEDs)
        0x19, 0x01, //   Usage Minimum (1)
        0x29, 0x05, //   Usage Maximum (5)
        0x91, 0x02, //   Output (Data, Variable, Absolute): LEDs
        0x95, 0x01, //   Report Count (1)
        0x75, 0x03, //   Report Size (3)
        0x91, 0x01, //   Output (Constant): LED padding
        0x95, 0x06, //   Report Count (6)
        0x75, 0x08, //   Report Size (8)
        0x15, 0x00, //   Logical Minimum (0)
        0x25, 0x65, //   Logical Maximum (101)
        0x05, 0x07, //   Usage Page (Key Codes)
        0x19, 0x00, //   Usage Minimum (0)
        0x29, 0x65, //   Usage Maximum (101)
        0x81, 0x00, //   Input (Data, Array): six key slots
        0xC0, // End Collection
    ];

    /// A three-button mouse with X and Y deltas: 3 + 5 bits, then two signed bytes.
    /// X and Y are one Input item with a count of 2.
    const MOUSE: [u8; 50] = [
        0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, // Generic Desktop, Mouse, Collection
        0x09, 0x01, 0xA1, 0x00, //   Pointer, Collection
        0x05, 0x09, 0x19, 0x01, 0x29, 0x03, // Buttons 1..3
        0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, // three button bits
        0x95, 0x01, 0x75, 0x05, 0x81, 0x01, // five padding bits
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, // X, Y
        0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06, // two relative bytes
        0xC0, 0xC0,
    ];

    /// The same keyboard, with a Report ID of 1 before its input.
    const KEYBOARD_WITH_ID: [u8; 65] = {
        let mut out = [0u8; 65];
        // Report ID 1 goes right after the Application collection opens.
        let mut i = 0;
        let mut o = 0;
        while i < BOOT_KEYBOARD.len() {
            out[o] = BOOT_KEYBOARD[i];
            o += 1;
            if i == 5 {
                out[o] = 0x85;
                out[o + 1] = 0x01;
                o += 2;
            }
            i += 1;
        }
        out
    };

    #[test]
    fn boot_keyboard_descriptor_has_the_boot_layout() {
        assert_eq!(BOOT_KEYBOARD.len(), 63);
        let map = ReportMap::parse(&BOOT_KEYBOARD).unwrap();
        let fields = map.fields();
        // Input items: modifiers, reserved byte (constant), six key slots.
        assert_eq!(fields.len(), 3);
        let modifiers = fields[0];
        assert_eq!(
            (modifiers.bit_offset, modifiers.size, modifiers.count),
            (0, 1, 8)
        );
        assert_eq!(modifiers.usage_page, PAGE_KEYBOARD);
        assert_eq!((modifiers.usage_min, modifiers.usage_max), (0xE0, 0xE7));
        assert!(modifiers.is_variable());
        let reserved = fields[1];
        assert!(reserved.is_constant());
        assert_eq!((reserved.bit_offset, reserved.total_bits()), (8, 8));
        let keys = fields[2];
        assert_eq!((keys.bit_offset, keys.size, keys.count), (16, 8, 6));
        assert_eq!((keys.usage_min, keys.usage_max), (0, 0x65));
        assert!(!keys.is_variable());
        assert_eq!(map.report_len(0), Some(8));
        assert!(!map.uses_report_ids());
    }

    #[test]
    fn report_id_descriptor_offsets_and_split() {
        let map = ReportMap::parse(&KEYBOARD_WITH_ID).unwrap();
        assert!(map.uses_report_ids());
        assert!(map.fields().iter().all(|f| f.report_id == 1));
        assert_eq!(map.report_len(1), Some(8));
        let report = [0x01, 0x02, 0x00, 0x04, 0, 0, 0, 0, 0];
        let (id, data) = map.split(&report).unwrap();
        assert_eq!(id, 1);
        assert_eq!(data.len(), 8);
        let mods = map.fields()[0];
        assert_eq!(mods.value(data, 1), Some(1));
    }

    #[test]
    fn keyboard_report_decodes_modifiers_and_keys() {
        // Left Shift, reserved 0, then 'a' (0x04) in the first slot.
        let r = KeyboardReport::parse(&[0x02, 0, 0x04, 0, 0, 0, 0, 0]).unwrap();
        assert!(r.modifiers.shift());
        assert!(!r.modifiers.ctrl());
        assert_eq!(r.pressed().as_slice(), &[0x04]);
        assert_eq!(KeyboardReport::parse(&[0; 7]), Err(HidError::Truncated));
    }

    #[test]
    fn pressing_a_then_shift_a_types_a_then_capital_a() {
        let idle = KeyboardReport::default();
        let a = KeyboardReport::parse(&[0, 0, 0x04, 0, 0, 0, 0, 0]).unwrap();
        let presses = new_presses(&idle, &a);
        assert_eq!(presses.as_slice(), &[0x04]);
        assert_eq!(usage_to_char(0x04, a.modifiers.shift()), Some('a'));

        // Shift goes down first (no key yet), then 'a' is pressed with it held.
        let shift = KeyboardReport::parse(&[0x02, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(new_presses(&idle, &shift).as_slice().is_empty());
        let shift_a = KeyboardReport::parse(&[0x02, 0, 0x04, 0, 0, 0, 0, 0]).unwrap();
        let presses = new_presses(&shift, &shift_a);
        assert_eq!(presses.as_slice(), &[0x04]);
        assert_eq!(
            usage_to_char(presses.as_slice()[0], shift_a.modifiers.shift()),
            Some('A')
        );
    }

    #[test]
    fn rolling_keys_reports_only_the_new_one() {
        let a = KeyboardReport::parse(&[0, 0, 0x04, 0x05, 0, 0, 0, 0]).unwrap();
        // 'a' lifts, 'b' stays, and 'c' (0x06) goes down.
        let b = KeyboardReport::parse(&[0, 0, 0x05, 0x06, 0, 0, 0, 0]).unwrap();
        assert_eq!(new_presses(&a, &b).as_slice(), &[0x06]);
    }

    #[test]
    fn six_key_roll_over_is_detected_and_reports_nothing() {
        let idle = KeyboardReport::default();
        let rollover = KeyboardReport::parse(&[0, 0, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01]).unwrap();
        assert!(rollover.rollover());
        assert!(rollover.pressed().as_slice().is_empty());
        assert!(new_presses(&idle, &rollover).as_slice().is_empty());
        // Six real keys fill the slots without roll-over.
        let six = KeyboardReport::parse(&[0, 0, 4, 5, 6, 7, 8, 9]).unwrap();
        assert!(!six.rollover());
        assert_eq!(six.pressed().as_slice().len(), 6);
    }

    #[test]
    fn usage_table_covers_letters_digits_and_punctuation() {
        assert_eq!(usage_to_char(0x04, false), Some('a'));
        assert_eq!(usage_to_char(0x1D, true), Some('Z'));
        assert_eq!(usage_to_char(0x1E, false), Some('1'));
        assert_eq!(usage_to_char(0x1E, true), Some('!'));
        assert_eq!(usage_to_char(0x27, false), Some('0'));
        assert_eq!(usage_to_char(0x27, true), Some(')'));
        assert_eq!(usage_to_char(0x2C, false), Some(' '));
        assert_eq!(usage_to_char(0x33, true), Some(':'));
        assert_eq!(usage_to_char(0x38, false), Some('/'));
        assert_eq!(usage_to_char(0x28, false), Some('\n'));
        // Modifiers and function keys type nothing.
        assert_eq!(usage_to_char(0xE1, false), None);
        assert_eq!(usage_to_char(0x3A, false), None);
    }

    #[test]
    fn mouse_report_decodes_buttons_and_signed_deltas() {
        let m = MouseReport::parse(&[0x05, 0xFF, 0x10]).unwrap();
        assert!(m.left());
        assert!(!m.right());
        assert!(m.middle());
        assert_eq!((m.dx, m.dy), (-1, 16));
        assert_eq!(MouseReport::parse(&[0]), Err(HidError::Truncated));
    }

    #[test]
    fn mouse_descriptor_marks_deltas_relative_and_signed() {
        let map = ReportMap::parse(&MOUSE).unwrap();
        let fields = map.fields();
        // Three button bits, five padding bits, then X and Y as one item of two.
        assert_eq!(fields.len(), 3);
        assert_eq!((fields[0].bit_offset, fields[0].count), (0, 3));
        assert!(fields[1].is_constant());
        assert_eq!(fields[1].bit_offset, 3);
        let xy = fields[2];
        assert!(xy.is_relative());
        assert_eq!((xy.bit_offset, xy.size, xy.count), (8, 8, 2));
        assert_eq!((xy.logical_min, xy.logical_max), (-127, 127));
        // No report ID, so the whole report is data: buttons, dx = -2, dy = 3.
        let report = [0x01, 0xFE, 0x03];
        assert_eq!(xy.value(&report, 0), Some(-2));
        assert_eq!(xy.value(&report, 1), Some(3));
    }

    #[test]
    fn unbalanced_and_malformed_descriptors_are_refused() {
        // End Collection with nothing open.
        assert_eq!(ReportMap::parse(&[0xC0]), Err(HidError::Unbalanced));
        // Collection never closed.
        assert_eq!(ReportMap::parse(&[0xA1, 0x01]), Err(HidError::Unbalanced));
        // Item says it has one data byte, but the input ends.
        assert_eq!(ReportMap::parse(&[0x05]), Err(HidError::Truncated));
        // Pop (global, tag 11) with an empty stack.
        assert_eq!(ReportMap::parse(&[0xB4]), Err(HidError::StackUnderflow));
        // Reserved main item type (tag 0xF).
        assert_eq!(ReportMap::parse(&[0xF0]), Err(HidError::BadItem));
    }

    #[test]
    fn push_and_pop_restore_globals() {
        // Report Size 8, Push, Report Size 1, Pop, then Input with 2 bytes.
        let d: Vec<u8> = [0x75, 0x08, 0xA4, 0x75, 0x01, 0xB4, 0x95, 0x01, 0x81, 0x02].to_vec();
        let map = ReportMap::parse(&d).unwrap();
        assert_eq!(map.fields()[0].size, 8);
        assert_eq!(map.fields()[0].count, 1);
    }

    #[test]
    fn too_many_fields_is_refused() {
        // MAX_FIELDS + 1 inputs of one bit each.
        let mut d: Vec<u8> = std::vec![0x75, 0x01, 0x95, 0x01];
        for _ in 0..=MAX_FIELDS {
            d.extend_from_slice(&[0x81, 0x02]);
        }
        assert_eq!(ReportMap::parse(&d), Err(HidError::TooManyFields));
    }

    #[test]
    fn bits_at_reads_across_bytes() {
        let data = [0b1010_0101u8, 0b0000_0011];
        assert_eq!(bits_at(&data, 0, 8), Some(0xA5));
        assert_eq!(bits_at(&data, 6, 4), Some(0b1110));
        assert_eq!(bits_at(&data, 12, 8), None);
    }
}
