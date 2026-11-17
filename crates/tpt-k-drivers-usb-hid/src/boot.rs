// SPDX-License-Identifier: MIT OR Apache-2.0
//! Boot protocol reports: the 8-byte keyboard report and the 3-byte mouse
//! report that every USB HID host must accept without parsing a descriptor.
//!
//! Keyboard keys are HID Keyboard/Keypad page usages (page 0x07). The usage
//! `0x01` in a key slot means roll-over: more keys were pressed than the report
//! can hold, so the slots do not say which keys are down.

use crate::HidError;

/// Usage in a key slot that signals roll-over (more keys than slots).
pub const KEY_ERROR_ROLLOVER: u8 = 0x01;

/// Number of key slots in a boot keyboard report.
pub const KEY_SLOTS: usize = 6;

/// The modifier byte of a boot keyboard report, one bit per modifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// Left Control.
    pub const LEFT_CTRL: u8 = 1 << 0;
    /// Left Shift.
    pub const LEFT_SHIFT: u8 = 1 << 1;
    /// Left Alt.
    pub const LEFT_ALT: u8 = 1 << 2;
    /// Left GUI (Windows or Command).
    pub const LEFT_GUI: u8 = 1 << 3;
    /// Right Control.
    pub const RIGHT_CTRL: u8 = 1 << 4;
    /// Right Shift.
    pub const RIGHT_SHIFT: u8 = 1 << 5;
    /// Right Alt.
    pub const RIGHT_ALT: u8 = 1 << 6;
    /// Right GUI.
    pub const RIGHT_GUI: u8 = 1 << 7;

    /// Modifiers from the raw byte.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// The raw byte.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// True if any of the bits in `mask` is set.
    #[must_use]
    pub const fn any(self, mask: u8) -> bool {
        self.0 & mask != 0
    }

    /// True if either Shift key is down.
    #[must_use]
    pub const fn shift(self) -> bool {
        self.any(Self::LEFT_SHIFT | Self::RIGHT_SHIFT)
    }

    /// True if either Control key is down.
    #[must_use]
    pub const fn ctrl(self) -> bool {
        self.any(Self::LEFT_CTRL | Self::RIGHT_CTRL)
    }
}

/// A decoded boot keyboard report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardReport {
    /// Modifier keys held.
    pub modifiers: Modifiers,
    /// Key usages held, with 0 for an empty slot.
    pub keys: [u8; KEY_SLOTS],
}

impl KeyboardReport {
    /// Decodes an 8-byte boot keyboard report. The reserved byte is ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, HidError> {
        if bytes.len() < 8 {
            return Err(HidError::Truncated);
        }
        let mut keys = [0u8; KEY_SLOTS];
        keys.copy_from_slice(&bytes[2..8]);
        Ok(Self {
            modifiers: Modifiers::from_bits(bytes[0]),
            keys,
        })
    }

    /// True when the report is a roll-over error: its keys cannot be trusted.
    #[must_use]
    pub fn rollover(&self) -> bool {
        self.keys.contains(&KEY_ERROR_ROLLOVER)
    }

    /// The key usages held, in slot order, without empty or error slots.
    #[must_use]
    pub fn pressed(&self) -> KeyList {
        let mut list = KeyList::default();
        if self.rollover() {
            return list;
        }
        for &k in self.keys.iter().filter(|k| **k != 0) {
            list.push(k);
        }
        list
    }
}

/// A small fixed list of key usages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyList {
    keys: [u8; KEY_SLOTS],
    len: usize,
}

impl KeyList {
    fn push(&mut self, key: u8) {
        if self.len < KEY_SLOTS {
            self.keys[self.len] = key;
            self.len += 1;
        }
    }

    /// The usages as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.keys[..self.len]
    }

    /// True if `key` is in the list.
    #[must_use]
    pub fn contains(&self, key: u8) -> bool {
        self.as_slice().contains(&key)
    }
}

/// The keys that went down between two reports, in slot order of `cur`.
///
/// If `cur` is a roll-over error, nothing is reported: the slots do not show
/// which keys are down, so a key press cannot be attributed.
#[must_use]
pub fn new_presses(prev: &KeyboardReport, cur: &KeyboardReport) -> KeyList {
    let mut out = KeyList::default();
    if cur.rollover() {
        return out;
    }
    let before = prev.pressed();
    for &k in cur.pressed().as_slice() {
        if !before.contains(k) {
            out.push(k);
        }
    }
    out
}

/// The character a key usage types, for the US layout. `shift` selects the
/// shifted character. Returns `None` for keys that type nothing (modifiers,
/// function keys, navigation) and for keys this table does not cover.
#[must_use]
pub fn usage_to_char(usage: u8, shift: bool) -> Option<char> {
    const DIGITS: &[u8; 10] = b"1234567890";
    const SHIFTED_DIGITS: &[u8; 10] = b"!@#$%^&*()";
    match usage {
        0x04..=0x1D => {
            let base = b'a' + (usage - 0x04);
            Some(
                (if shift {
                    base.to_ascii_uppercase()
                } else {
                    base
                }) as char,
            )
        }
        0x1E..=0x27 => {
            let i = (usage - 0x1E) as usize;
            Some((if shift { SHIFTED_DIGITS[i] } else { DIGITS[i] }) as char)
        }
        0x28 => Some('\n'),
        0x2B => Some('\t'),
        0x2C => Some(' '),
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        _ => None,
    }
}

/// A decoded boot mouse report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseReport {
    /// Button bits: bit 0 left, bit 1 right, bit 2 middle.
    pub buttons: u8,
    /// Horizontal movement since the last report, positive to the right.
    pub dx: i8,
    /// Vertical movement since the last report, positive downward.
    pub dy: i8,
}

impl MouseReport {
    /// Decodes a boot mouse report of at least 3 bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, HidError> {
        if bytes.len() < 3 {
            return Err(HidError::Truncated);
        }
        Ok(Self {
            buttons: bytes[0] & 0x07,
            dx: bytes[1] as i8,
            dy: bytes[2] as i8,
        })
    }

    /// True if the left button is down.
    #[must_use]
    pub const fn left(&self) -> bool {
        self.buttons & 0x01 != 0
    }

    /// True if the right button is down.
    #[must_use]
    pub const fn right(&self) -> bool {
        self.buttons & 0x02 != 0
    }

    /// True if the middle button is down.
    #[must_use]
    pub const fn middle(&self) -> bool {
        self.buttons & 0x04 != 0
    }
}
