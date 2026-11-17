//! String operations over NUL-terminated byte slices.
//!
//! A string ends at its first NUL byte. If the slice holds no NUL, the whole
//! slice is the string. Indices returned are positions in the slice.

/// Length of the string in `s`, looking at no more than `max` bytes.
#[must_use]
pub fn strnlen(s: &[u8], max: usize) -> usize {
    let limit = s.len().min(max);
    s[..limit].iter().position(|&b| b == 0).unwrap_or(limit)
}

/// Length of the string in `s`.
#[must_use]
pub fn strlen(s: &[u8]) -> usize {
    strnlen(s, s.len())
}

/// Compares up to `n` bytes of two strings, as C's `strncmp` does.
///
/// Returns the difference of the first differing bytes, or `0` when the
/// strings are equal up to `n` or to their NUL terminators.
#[must_use]
pub fn strncmp(a: &[u8], b: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return i32::from(x) - i32::from(y);
        }
        if x == 0 {
            return 0;
        }
    }
    0
}

/// Compares two strings, as C's `strcmp` does.
#[must_use]
pub fn strcmp(a: &[u8], b: &[u8]) -> i32 {
    strncmp(a, b, usize::MAX)
}

/// First occurrence of `c` in the string. Like C, searching for `0` finds the
/// terminator, at the string's length.
#[must_use]
pub fn strchr(s: &[u8], c: u8) -> Option<usize> {
    let len = strlen(s);
    if c == 0 {
        return Some(len);
    }
    s[..len].iter().position(|&b| b == c)
}

/// Last occurrence of `c` in the string. Searching for `0` finds the terminator.
#[must_use]
pub fn strrchr(s: &[u8], c: u8) -> Option<usize> {
    let len = strlen(s);
    if c == 0 {
        return Some(len);
    }
    s[..len].iter().rposition(|&b| b == c)
}

/// First position of `needle` within `hay`. An empty needle matches at 0.
#[must_use]
pub fn strstr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let h = strlen(hay);
    let n = strlen(needle);
    if n == 0 {
        return Some(0);
    }
    if n > h {
        return None;
    }
    (0..=h - n).find(|&i| hay[i..i + n] == needle[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_stop_at_nul_or_limit() {
        assert_eq!(strlen(b"hello\0world"), 5);
        assert_eq!(strlen(b""), 0);
        assert_eq!(strlen(b"\0"), 0);
        assert_eq!(strlen(b"no nul"), 6);
        assert_eq!(strnlen(b"hello\0", 3), 3);
        assert_eq!(strnlen(b"hi\0", 10), 2);
    }

    #[test]
    fn comparisons() {
        assert_eq!(strcmp(b"abc\0", b"abc\0"), 0);
        assert_eq!(strcmp(b"abd\0", b"abc\0"), 1);
        assert_eq!(strcmp(b"ab\0", b"abc\0"), -i32::from(b'c'));
        assert_eq!(strcmp(b"\0", b"\0"), 0);
        assert_eq!(strcmp(b"", b"a"), -i32::from(b'a'));
        assert_eq!(strncmp(b"abcX\0", b"abcY\0", 3), 0);
        assert_eq!(
            strncmp(b"abcX\0", b"abcY\0", 4),
            i32::from(b'X') - i32::from(b'Y')
        );
        assert_eq!(strncmp(b"ab\0", b"ac\0", 0), 0);
    }

    #[test]
    fn character_search() {
        assert_eq!(strchr(b"hello\0", b'l'), Some(2));
        assert_eq!(strchr(b"hello\0", b'z'), None);
        assert_eq!(strchr(b"hello\0", 0), Some(5));
        assert_eq!(strrchr(b"hello\0", b'l'), Some(3));
        assert_eq!(strrchr(b"hello\0junk l\0", b'l'), Some(3));
        assert_eq!(strrchr(b"hello\0", 0), Some(5));
        // Bytes after the NUL are not part of the string.
        assert_eq!(strchr(b"ab\0c\0", b'c'), None);
    }

    #[test]
    fn substring_search() {
        assert_eq!(strstr(b"needle in hay\0", b"in\0"), Some(7));
        assert_eq!(strstr(b"abc\0", b"abcd\0"), None);
        assert_eq!(strstr(b"abc\0", b"\0"), Some(0));
        assert_eq!(strstr(b"aaab\0", b"aab\0"), Some(1));
        assert_eq!(strstr(b"\0", b"a\0"), None);
    }
}
