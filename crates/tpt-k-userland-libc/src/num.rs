//! Integer parsing in the style of `strtol`.

/// Why a number could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumError {
    /// The base is neither 0 nor in 2..=36.
    BadBase,
    /// No digits follow the optional whitespace and sign.
    NoDigits,
    /// The value does not fit in an `i64`.
    Overflow,
}

/// The value of an ASCII digit or letter in bases up to 36.
fn digit_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'z' => Some(c - b'a' + 10),
        b'A'..=b'Z' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parses an integer from the start of `s`, as C's `strtol` does.
///
/// Leading space, tab, newline, carriage return, vertical tab and form feed
/// are skipped, then an optional `+` or `-`. Base 0 picks the base from the
/// prefix: `0x` for 16, a leading `0` for 8, otherwise 10. Base 16 also accepts
/// a `0x` prefix. A `0x` not followed by a hex digit is read as the digit `0`.
///
/// On success returns the value and the index just past the last digit. The
/// value is accumulated as a negative number when the sign is `-`, so
/// `i64::MIN` parses.
pub fn strtol(s: &[u8], base: u32) -> Result<(i64, usize), NumError> {
    if base == 1 || base > 36 {
        return Err(NumError::BadBase);
    }
    let mut i = 0;
    while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)) {
        i += 1;
    }
    let mut negative = false;
    if let Some(&sign @ (b'+' | b'-')) = s.get(i) {
        negative = sign == b'-';
        i += 1;
    }

    let mut base = base;
    let hex_prefix = matches!(s.get(i..i + 2), Some([b'0', b'x' | b'X']))
        && s.get(i + 2)
            .and_then(|&c| digit_value(c))
            .is_some_and(|d| d < 16);
    if (base == 0 || base == 16) && hex_prefix {
        base = 16;
        i += 2;
    } else if base == 0 {
        base = if s.get(i) == Some(&b'0') { 8 } else { 10 };
    }

    let radix = i64::from(base);
    let start = i;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(d) = s.get(i).and_then(|&c| digit_value(c)) {
        if i64::from(d) >= radix {
            break;
        }
        i += 1;
        let digit = i64::from(d);
        let next = if negative {
            value.checked_mul(radix).and_then(|v| v.checked_sub(digit))
        } else {
            value.checked_mul(radix).and_then(|v| v.checked_add(digit))
        };
        match next {
            Some(v) => value = v,
            None => overflow = true,
        }
    }

    if i == start {
        return Err(NumError::NoDigits);
    }
    if overflow {
        return Err(NumError::Overflow);
    }
    Ok((value, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_with_whitespace_and_sign() {
        assert_eq!(strtol(b"  +42xyz", 10), Ok((42, 5)));
        assert_eq!(strtol(b"\t-17", 10), Ok((-17, 4)));
        assert_eq!(strtol(b"0", 10), Ok((0, 1)));
    }

    #[test]
    fn base_zero_detects_prefix() {
        assert_eq!(strtol(b"0x1F", 0), Ok((31, 4)));
        assert_eq!(strtol(b"0X1f", 0), Ok((31, 4)));
        assert_eq!(strtol(b"017", 0), Ok((15, 3)));
        assert_eq!(strtol(b"19", 0), Ok((19, 2)));
        // Digit 8 is not an octal digit, so parsing stops before it.
        assert_eq!(strtol(b"08", 0), Ok((0, 1)));
    }

    #[test]
    fn explicit_bases_and_letters() {
        assert_eq!(strtol(b"ff", 16), Ok((255, 2)));
        assert_eq!(strtol(b"0xff", 16), Ok((255, 4)));
        assert_eq!(strtol(b"777", 8), Ok((511, 3)));
        assert_eq!(strtol(b"z", 36), Ok((35, 1)));
        assert_eq!(strtol(b"1010", 2), Ok((10, 4)));
        assert_eq!(strtol(b"12", 2), Ok((1, 1)));
    }

    #[test]
    fn hex_prefix_without_digit_reads_zero() {
        assert_eq!(strtol(b"0x", 16), Ok((0, 1)));
        assert_eq!(strtol(b"0xg", 0), Ok((0, 1)));
    }

    #[test]
    fn no_digits_is_an_error() {
        assert_eq!(strtol(b"", 10), Err(NumError::NoDigits));
        assert_eq!(strtol(b"   ", 10), Err(NumError::NoDigits));
        assert_eq!(strtol(b"-", 10), Err(NumError::NoDigits));
        assert_eq!(strtol(b"x1", 10), Err(NumError::NoDigits));
    }

    #[test]
    fn bad_base_is_an_error() {
        assert_eq!(strtol(b"1", 1), Err(NumError::BadBase));
        assert_eq!(strtol(b"1", 37), Err(NumError::BadBase));
    }

    #[test]
    fn i64_boundaries() {
        assert_eq!(strtol(b"9223372036854775807", 10), Ok((i64::MAX, 19)));
        assert_eq!(strtol(b"-9223372036854775808", 10), Ok((i64::MIN, 20)));
        assert_eq!(strtol(b"9223372036854775808", 10), Err(NumError::Overflow));
        assert_eq!(strtol(b"-9223372036854775809", 10), Err(NumError::Overflow));
        assert_eq!(strtol(b"0xffffffffffffffffff", 16), Err(NumError::Overflow));
    }
}
