// SPDX-License-Identifier: MIT OR Apache-2.0
//! HID report descriptor parser (HID 1.11, section 6.2.2).
//!
//! The parser walks the short items and records each `Input` main item as a
//! [`Field`]: where its bits sit in the report, how wide they are, and what the
//! usage and logical range are. Storage is fixed: at most [`MAX_FIELDS`] fields
//! and [`MAX_REPORTS`] report IDs. Long items are skipped. `Output` and
//! `Feature` items take no input bits and are only parsed.
//!
//! A local `Usage` list is reduced to one range: `Usage Minimum`/`Usage
//! Maximum` when both are given, otherwise the first `Usage` for both ends.
//! Descriptors that map one usage per item in a variable field are therefore
//! not decoded per usage. Boot-protocol descriptors do not need that.

use crate::HidError;

/// Most input fields one descriptor may declare.
pub const MAX_FIELDS: usize = 32;
/// Most distinct report IDs one descriptor may declare (report ID 0 counts as
/// one when used).
pub const MAX_REPORTS: usize = 8;
const MAX_PUSH: usize = 4;

/// Input main item flag: bit 0 set means constant (padding).
pub const FLAG_CONSTANT: u32 = 1 << 0;
/// Input main item flag: bit 1 set means variable (one value per field).
pub const FLAG_VARIABLE: u32 = 1 << 1;
/// Input main item flag: bit 2 set means relative (for example mouse deltas).
pub const FLAG_RELATIVE: u32 = 1 << 2;

/// One input field of a report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Field {
    /// Report ID this field belongs to, or 0 when the descriptor does not use IDs.
    pub report_id: u8,
    /// Bit offset of the field's first item, from the start of the report data
    /// (after the report ID byte, if any).
    pub bit_offset: u32,
    /// Width of one item in bits.
    pub size: u32,
    /// Number of items.
    pub count: u32,
    /// Usage page the usages belong to.
    pub usage_page: u16,
    /// Lowest usage in the field.
    pub usage_min: u16,
    /// Highest usage in the field.
    pub usage_max: u16,
    /// Logical minimum value.
    pub logical_min: i32,
    /// Logical maximum value.
    pub logical_max: i32,
    /// Input flags (`FLAG_*`).
    pub flags: u32,
}

impl Field {
    /// True for padding that carries no data.
    #[must_use]
    pub const fn is_constant(&self) -> bool {
        self.flags & FLAG_CONSTANT != 0
    }

    /// True for a field that carries one value per item (not an array of usages).
    #[must_use]
    pub const fn is_variable(&self) -> bool {
        self.flags & FLAG_VARIABLE != 0
    }

    /// True for a relative field, such as a mouse delta.
    #[must_use]
    pub const fn is_relative(&self) -> bool {
        self.flags & FLAG_RELATIVE != 0
    }

    /// Total width of the field in bits.
    #[must_use]
    pub const fn total_bits(&self) -> u32 {
        self.size * self.count
    }

    /// Reads item `index` from `data`, the report data after any report ID.
    /// Negative logical minimums sign-extend the value. Returns `None` if the
    /// index or the bits lie outside `data`.
    #[must_use]
    pub fn value(&self, data: &[u8], index: u32) -> Option<i32> {
        if index >= self.count || self.size == 0 || self.size > 32 {
            return None;
        }
        let raw = bits_at(data, self.bit_offset + index * self.size, self.size)?;
        let mut v = i64::from(raw);
        if self.logical_min < 0 && self.size < 32 && raw & (1 << (self.size - 1)) != 0 {
            v -= 1i64 << self.size;
        }
        i32::try_from(v).ok()
    }
}

/// Reads `size` bits (at most 32) starting at bit `bit` of `data`, least
/// significant bit first.
#[must_use]
pub fn bits_at(data: &[u8], bit: u32, size: u32) -> Option<u32> {
    if size > 32 {
        return None;
    }
    let end = bit.checked_add(size)?;
    if (end as usize) > data.len() * 8 {
        return None;
    }
    let mut v = 0u32;
    for k in 0..size {
        let b = bit + k;
        let byte = data[(b / 8) as usize];
        v |= u32::from((byte >> (b % 8)) & 1) << k;
    }
    Some(v)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Globals {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct Locals {
    usage_first: Option<u16>,
    usage_min: Option<u16>,
    usage_max: Option<u16>,
}

/// The input layout of a report descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReportMap {
    fields: [Field; MAX_FIELDS],
    len: usize,
    /// Per report ID (index 0..reports): the ID and its total input bits.
    sizes: [(u8, u32); MAX_REPORTS],
    reports: usize,
}

impl ReportMap {
    /// Parses a report descriptor.
    pub fn parse(desc: &[u8]) -> Result<Self, HidError> {
        let mut map = Self::default();
        let mut g = Globals::default();
        let mut stack = [Globals::default(); MAX_PUSH];
        let mut sp = 0usize;
        let mut l = Locals::default();
        let mut depth = 0u32;
        let mut i = 0usize;

        while i < desc.len() {
            let prefix = desc[i];
            if prefix == 0xFE {
                // Long item: prefix, data size, long tag, then data.
                let n = *desc.get(i + 1).ok_or(HidError::Truncated)? as usize;
                i += 3 + n;
                continue;
            }
            let size = match prefix & 0x3 {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            let kind = (prefix >> 2) & 0x3;
            let tag = prefix >> 4;
            let data = desc.get(i + 1..i + 1 + size).ok_or(HidError::Truncated)?;
            i += 1 + size;
            let u = unsigned(data);
            let s = signed(data);

            match (kind, tag) {
                // Main items.
                (0, 8) => {
                    map.add_input(&g, &l, u)?;
                    l = Locals::default();
                }
                (0, 9) | (0, 11) => {
                    // Output and Feature take no input bits.
                    l = Locals::default();
                }
                (0, 10) => {
                    depth += 1;
                    l = Locals::default();
                }
                (0, 12) => {
                    depth = depth.checked_sub(1).ok_or(HidError::Unbalanced)?;
                    l = Locals::default();
                }
                (0, _) => return Err(HidError::BadItem),
                // Global items.
                (1, 0) => g.usage_page = u as u16,
                (1, 1) => g.logical_min = s,
                (1, 2) => g.logical_max = s,
                (1, 7) => g.report_size = u,
                (1, 8) => g.report_id = u as u8,
                (1, 9) => g.report_count = u,
                (1, 10) => {
                    if sp == MAX_PUSH {
                        return Err(HidError::StackOverflow);
                    }
                    stack[sp] = g;
                    sp += 1;
                }
                (1, 11) => {
                    sp = sp.checked_sub(1).ok_or(HidError::StackUnderflow)?;
                    g = stack[sp];
                }
                // Global items this parser does not use (physical range, unit).
                (1, 3..=6) => {}
                (1, _) => return Err(HidError::BadItem),
                // Local items.
                (2, 0) => {
                    l.usage_first.get_or_insert(u as u16);
                }
                (2, 1) => l.usage_min = Some(u as u16),
                (2, 2) => l.usage_max = Some(u as u16),
                // Designators, strings and delimiters do not affect the layout.
                (2, _) => {}
                _ => return Err(HidError::BadItem),
            }
        }
        if depth != 0 {
            return Err(HidError::Unbalanced);
        }
        Ok(map)
    }

    fn add_input(&mut self, g: &Globals, l: &Locals, flags: u32) -> Result<(), HidError> {
        if self.len == MAX_FIELDS {
            return Err(HidError::TooManyFields);
        }
        let bits = g
            .report_size
            .checked_mul(g.report_count)
            .ok_or(HidError::ReportTooLarge)?;
        let idx = self.report_index(g.report_id)?;
        let offset = self.sizes[idx].1;
        self.sizes[idx].1 = offset.checked_add(bits).ok_or(HidError::ReportTooLarge)?;

        let (usage_min, usage_max) = match (l.usage_min, l.usage_max) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                let u = l.usage_first.unwrap_or(0);
                (u, u)
            }
        };
        self.fields[self.len] = Field {
            report_id: g.report_id,
            bit_offset: offset,
            size: g.report_size,
            count: g.report_count,
            usage_page: g.usage_page,
            usage_min,
            usage_max,
            logical_min: g.logical_min,
            logical_max: g.logical_max,
            flags,
        };
        self.len += 1;
        Ok(())
    }

    fn report_index(&mut self, id: u8) -> Result<usize, HidError> {
        if let Some(i) = self.sizes[..self.reports]
            .iter()
            .position(|(r, _)| *r == id)
        {
            return Ok(i);
        }
        if self.reports == MAX_REPORTS {
            return Err(HidError::TooManyReports);
        }
        self.sizes[self.reports] = (id, 0);
        self.reports += 1;
        Ok(self.reports - 1)
    }

    /// The input fields, in descriptor order.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields[..self.len]
    }

    /// The fields of one report ID, in descriptor order.
    pub fn fields_for(&self, report_id: u8) -> impl Iterator<Item = &Field> {
        self.fields()
            .iter()
            .filter(move |f| f.report_id == report_id)
    }

    /// True when the descriptor declares report IDs, so each report starts with
    /// an ID byte.
    #[must_use]
    pub fn uses_report_ids(&self) -> bool {
        self.fields().iter().any(|f| f.report_id != 0)
    }

    /// Length in bytes of an input report with `report_id`, or `None` if the
    /// descriptor declares no input for that ID. The ID byte is not counted.
    #[must_use]
    pub fn report_len(&self, report_id: u8) -> Option<usize> {
        self.sizes[..self.reports]
            .iter()
            .find(|(r, _)| *r == report_id)
            .map(|(_, bits)| bits.div_ceil(8) as usize)
    }

    /// Splits a received report into its ID and its data. Without report IDs
    /// the ID is 0 and the whole report is data.
    #[must_use]
    pub fn split<'a>(&self, report: &'a [u8]) -> Option<(u8, &'a [u8])> {
        if self.uses_report_ids() {
            let (id, data) = report.split_first()?;
            Some((*id, data))
        } else {
            Some((0, report))
        }
    }
}

fn unsigned(data: &[u8]) -> u32 {
    data.iter()
        .enumerate()
        .fold(0u32, |acc, (k, b)| acc | (u32::from(*b) << (8 * k)))
}

fn signed(data: &[u8]) -> i32 {
    match data.len() {
        1 => i32::from(data[0] as i8),
        2 => i32::from(i16::from_le_bytes([data[0], data[1]])),
        4 => i32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        _ => 0,
    }
}
