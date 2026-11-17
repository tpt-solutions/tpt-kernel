//! Memory operations over byte slices.

/// Copies the shorter of `src` and `dst` into the start of `dst`.
///
/// Returns the number of bytes copied. The slices cannot overlap, because the
/// borrow rules keep `dst` and `src` apart; use [`memmove`] for overlap within
/// one buffer.
pub fn memcpy(dst: &mut [u8], src: &[u8]) -> usize {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

/// Moves `len` bytes inside `buf` from offset `src` to offset `dst`.
///
/// Overlapping ranges are handled as if the source were copied through a
/// temporary. Returns `None`, and changes nothing, if either range leaves `buf`.
pub fn memmove(buf: &mut [u8], dst: usize, src: usize, len: usize) -> Option<()> {
    let src_end = src.checked_add(len)?;
    let dst_end = dst.checked_add(len)?;
    if src_end > buf.len() || dst_end > buf.len() {
        return None;
    }
    buf.copy_within(src..src_end, dst);
    Some(())
}

/// Sets every byte of `dst` to `value`.
pub fn memset(dst: &mut [u8], value: u8) {
    dst.fill(value);
}

/// Compares the common prefix of `a` and `b`.
///
/// Returns the difference of the first pair of bytes that differ, as C's
/// `memcmp` does, or `0` if the common prefix is equal. Bytes past the shorter
/// slice are not compared.
#[must_use]
pub fn memcmp(a: &[u8], b: &[u8]) -> i32 {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return i32::from(*x) - i32::from(*y);
        }
    }
    0
}

/// Index of the first byte equal to `c`, if any.
#[must_use]
pub fn memchr(s: &[u8], c: u8) -> Option<usize> {
    s.iter().position(|&b| b == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memcpy_copies_the_shorter_length() {
        let mut dst = [0u8; 4];
        assert_eq!(memcpy(&mut dst, b"abcdef"), 4);
        assert_eq!(&dst, b"abcd");
        let mut dst = [0u8; 8];
        assert_eq!(memcpy(&mut dst, b"xy"), 2);
        assert_eq!(&dst[..2], b"xy");
        assert_eq!(memcpy(&mut [], b"abc"), 0);
    }

    #[test]
    fn memmove_handles_overlap_both_ways() {
        // Shift right: the ranges overlap and dst > src.
        let mut buf = *b"abcdef";
        assert_eq!(memmove(&mut buf, 2, 0, 4), Some(()));
        assert_eq!(&buf, b"ababcd");
        // Shift left: dst < src.
        let mut buf = *b"abcdef";
        assert_eq!(memmove(&mut buf, 0, 2, 4), Some(()));
        assert_eq!(&buf, b"cdefef");
    }

    #[test]
    fn memmove_refuses_out_of_range_and_wrapping_ranges() {
        let mut buf = [0u8; 4];
        assert_eq!(memmove(&mut buf, 0, 2, 3), None);
        assert_eq!(memmove(&mut buf, 3, 0, 2), None);
        assert_eq!(memmove(&mut buf, usize::MAX, 0, 2), None);
        assert_eq!(memmove(&mut buf, 0, 0, 0), Some(()));
    }

    #[test]
    fn memset_fills_and_empty_is_fine() {
        let mut buf = [1u8; 5];
        memset(&mut buf, 0xAB);
        assert_eq!(buf, [0xAB; 5]);
        memset(&mut [], 0);
    }

    #[test]
    fn memcmp_returns_first_difference() {
        assert_eq!(memcmp(b"abc", b"abc"), 0);
        assert_eq!(memcmp(b"abd", b"abc"), 1);
        assert_eq!(memcmp(b"abb", b"abc"), -1);
        assert_eq!(memcmp(b"", b"x"), 0);
        // High bytes compare as unsigned.
        assert_eq!(memcmp(&[0xFF], &[0x01]), 254);
    }

    #[test]
    fn memchr_finds_first_occurrence() {
        assert_eq!(memchr(b"hello", b'l'), Some(2));
        assert_eq!(memchr(b"hello", b'z'), None);
        assert_eq!(memchr(b"", b'a'), None);
    }
}
