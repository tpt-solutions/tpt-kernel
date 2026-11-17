// SPDX-License-Identifier: MIT OR Apache-2.0
//! Byte ranges to sector spans, with overflow-checked arithmetic.

/// Bytes per sector. The bridge works on 512-byte sectors only.
pub const SECTOR_SIZE: usize = 512;

/// The sectors a byte range touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Index of the first sector touched.
    pub first_sector: u64,
    /// Number of sectors touched. Zero for an empty range.
    pub sectors: u64,
    /// Byte offset of the range inside its first sector.
    pub start: usize,
}

/// The span of sectors that the byte range `offset .. offset + len` touches.
///
/// Returns `None` if `offset + len` overflows `u64`. An empty range touches no
/// sectors, but its `first_sector` and `start` still locate `offset`.
#[must_use]
pub fn span(offset: u64, len: u64) -> Option<Span> {
    let size = SECTOR_SIZE as u64;
    let first_sector = offset / size;
    let start = (offset % size) as usize;
    if len == 0 {
        return Some(Span {
            first_sector,
            sectors: 0,
            start,
        });
    }
    let end = offset.checked_add(len)?;
    let last_sector = (end - 1) / size;
    Some(Span {
        first_sector,
        sectors: last_sector - first_sector + 1,
        start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_and_unaligned_spans() {
        assert_eq!(
            span(0, 512),
            Some(Span {
                first_sector: 0,
                sectors: 1,
                start: 0
            })
        );
        // One byte at the end of sector 0 and one at the start of sector 1.
        assert_eq!(
            span(511, 2),
            Some(Span {
                first_sector: 0,
                sectors: 2,
                start: 511
            })
        );
        // Three full sectors starting at sector 2.
        assert_eq!(
            span(1024, 1536),
            Some(Span {
                first_sector: 2,
                sectors: 3,
                start: 0
            })
        );
    }

    #[test]
    fn empty_range_touches_no_sectors() {
        assert_eq!(
            span(700, 0),
            Some(Span {
                first_sector: 1,
                sectors: 0,
                start: 188
            })
        );
    }

    #[test]
    fn overflowing_range_is_refused() {
        assert_eq!(span(u64::MAX, 2), None);
        assert_eq!(span(u64::MAX, 1), None);
        // The last byte of the address space is still a valid range.
        assert!(span(u64::MAX - 1, 1).is_some());
    }
}
