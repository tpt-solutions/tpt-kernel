//! Integer formatting into a caller-supplied buffer, with a small subset of
//! `printf` conversions: `%d`, `%u`, `%x` and `%o`, with width, zero padding
//! and left alignment. Nothing is allocated.

/// The conversion to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    /// Decimal with a sign (`%d`). For [`format_u64`] this prints unsigned.
    Signed,
    /// Decimal without a sign (`%u`).
    Unsigned,
    /// Lowercase hexadecimal (`%x`).
    Hex,
    /// Octal (`%o`).
    Octal,
}

/// How one integer is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// The conversion.
    pub conv: Conv,
    /// Minimum field width in bytes. Shorter output is padded.
    pub width: usize,
    /// Pad with zeros after the sign instead of spaces. Ignored when
    /// `left_align` is set.
    pub zero_pad: bool,
    /// Pad on the right with spaces.
    pub left_align: bool,
}

impl Spec {
    /// A spec with the given conversion, no width, and no padding flags.
    #[must_use]
    pub const fn new(conv: Conv) -> Self {
        Self {
            conv,
            width: 0,
            zero_pad: false,
            left_align: false,
        }
    }

    /// The same spec with a minimum field width.
    #[must_use]
    pub const fn width(self, width: usize) -> Self {
        Self { width, ..self }
    }

    /// The same spec with zero padding enabled.
    #[must_use]
    pub const fn zero_pad(self) -> Self {
        Self {
            zero_pad: true,
            ..self
        }
    }

    /// The same spec with left alignment enabled.
    #[must_use]
    pub const fn left_align(self) -> Self {
        Self {
            left_align: true,
            ..self
        }
    }
}

/// Why formatting failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtError {
    /// The output does not fit in the buffer.
    Truncated,
}

/// Digit characters, indexed by value.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Writer<'_> {
    fn byte(&mut self, b: u8) -> Result<(), FmtError> {
        let slot = self.buf.get_mut(self.pos).ok_or(FmtError::Truncated)?;
        *slot = b;
        self.pos += 1;
        Ok(())
    }

    fn fill(&mut self, b: u8, count: usize) -> Result<(), FmtError> {
        for _ in 0..count {
            self.byte(b)?;
        }
        Ok(())
    }
}

/// Formats `magnitude` with the sign `sign` (empty or `-`) under `spec`.
fn emit(buf: &mut [u8], sign: &[u8], magnitude: u64, spec: Spec) -> Result<usize, FmtError> {
    let radix = match spec.conv {
        Conv::Hex => 16,
        Conv::Octal => 8,
        Conv::Signed | Conv::Unsigned => 10,
    };
    let mut tmp = [0u8; 22];
    let mut n = 0;
    let mut m = magnitude;
    loop {
        tmp[n] = DIGITS[(m % radix) as usize];
        n += 1;
        m /= radix;
        if m == 0 {
            break;
        }
    }
    // The digits were produced least significant first.
    tmp[..n].reverse();
    let digits = &tmp[..n];

    let used = sign.len() + n;
    let pad = spec.width.saturating_sub(used);
    let mut w = Writer { buf, pos: 0 };
    if spec.left_align {
        w.fill_bytes(sign)?;
        w.fill_bytes(digits)?;
        w.fill(b' ', pad)?;
    } else if spec.zero_pad {
        w.fill_bytes(sign)?;
        w.fill(b'0', pad)?;
        w.fill_bytes(digits)?;
    } else {
        w.fill(b' ', pad)?;
        w.fill_bytes(sign)?;
        w.fill_bytes(digits)?;
    }
    Ok(w.pos)
}

impl Writer<'_> {
    fn fill_bytes(&mut self, bytes: &[u8]) -> Result<(), FmtError> {
        for &b in bytes {
            self.byte(b)?;
        }
        Ok(())
    }
}

/// Formats an unsigned value. `Conv::Signed` prints it in decimal.
///
/// Returns the number of bytes written, or [`FmtError::Truncated`] if `buf` is
/// too small.
pub fn format_u64(buf: &mut [u8], value: u64, spec: Spec) -> Result<usize, FmtError> {
    emit(buf, b"", value, spec)
}

/// Formats a signed value.
///
/// Under `Conv::Signed` a negative value gets a leading `-`. The other
/// conversions print the two's-complement bit pattern as unsigned, as C's
/// `%x` and `%o` do for a negative `int`.
pub fn format_i64(buf: &mut [u8], value: i64, spec: Spec) -> Result<usize, FmtError> {
    match spec.conv {
        Conv::Signed if value < 0 => emit(buf, b"-", value.unsigned_abs(), spec),
        Conv::Signed => emit(buf, b"", value.unsigned_abs(), spec),
        Conv::Unsigned | Conv::Hex | Conv::Octal => emit(buf, b"", value as u64, spec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_u(value: u64, spec: Spec) -> std::string::String {
        let mut buf = [0u8; 64];
        let n = format_u64(&mut buf, value, spec).unwrap();
        std::string::String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    fn run_i(value: i64, spec: Spec) -> std::string::String {
        let mut buf = [0u8; 64];
        let n = format_i64(&mut buf, value, spec).unwrap();
        std::string::String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    #[test]
    fn plain_conversions() {
        assert_eq!(run_u(0, Spec::new(Conv::Unsigned)), "0");
        assert_eq!(run_u(255, Spec::new(Conv::Hex)), "ff");
        assert_eq!(run_u(8, Spec::new(Conv::Octal)), "10");
        assert_eq!(
            run_u(u64::MAX, Spec::new(Conv::Unsigned)),
            "18446744073709551615"
        );
        assert_eq!(run_i(-42, Spec::new(Conv::Signed)), "-42");
        assert_eq!(
            run_i(i64::MIN, Spec::new(Conv::Signed)),
            "-9223372036854775808"
        );
    }

    #[test]
    fn width_pads_with_spaces_by_default() {
        assert_eq!(run_u(42, Spec::new(Conv::Unsigned).width(5)), "   42");
        assert_eq!(
            run_u(42, Spec::new(Conv::Unsigned).width(5).left_align()),
            "42   "
        );
        // A width shorter than the output has no effect.
        assert_eq!(run_u(12345, Spec::new(Conv::Unsigned).width(2)), "12345");
    }

    #[test]
    fn zero_padding_goes_after_the_sign() {
        assert_eq!(
            run_u(0xff, Spec::new(Conv::Hex).width(6).zero_pad()),
            "0000ff"
        );
        assert_eq!(
            run_i(-42, Spec::new(Conv::Signed).width(6).zero_pad()),
            "-00042"
        );
        // Left alignment overrides zero padding.
        assert_eq!(
            run_i(
                -42,
                Spec::new(Conv::Signed).width(6).zero_pad().left_align()
            ),
            "-42   "
        );
    }

    #[test]
    fn negative_values_under_unsigned_conversions_are_bit_patterns() {
        assert_eq!(run_i(-1, Spec::new(Conv::Hex)), "ffffffffffffffff");
        assert_eq!(run_i(-1, Spec::new(Conv::Unsigned)), "18446744073709551615");
    }

    #[test]
    fn truncated_output_is_reported() {
        let mut small = [0u8; 3];
        assert_eq!(
            format_u64(&mut small, 12345, Spec::new(Conv::Unsigned)),
            Err(FmtError::Truncated)
        );
        let mut exact = [0u8; 5];
        assert_eq!(
            format_u64(&mut exact, 12345, Spec::new(Conv::Unsigned)),
            Ok(5)
        );
        let mut sign = [0u8; 2];
        assert_eq!(format_i64(&mut sign, -1, Spec::new(Conv::Signed)), Ok(2));
        assert_eq!(
            format_i64(&mut sign, -12, Spec::new(Conv::Signed)),
            Err(FmtError::Truncated)
        );
    }
}
