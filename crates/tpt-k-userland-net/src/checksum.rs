// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Internet checksum (RFC 1071) and the pseudo-header sums used by UDP and
//! TCP (RFC 768, RFC 793).

/// A running one's-complement sum. Bytes may be added in pieces of any length;
/// an odd byte at the end of one piece is paired with the first byte of the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checksum {
    sum: u32,
    pending: Option<u8>,
}

impl Checksum {
    /// An empty sum.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sum: 0,
            pending: None,
        }
    }

    /// Adds `bytes` to the sum, in network byte order.
    pub fn add(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        if let Some(hi) = self.pending.take() {
            match rest.split_first() {
                Some((&lo, tail)) => {
                    self.sum += u32::from(u16::from_be_bytes([hi, lo]));
                    rest = tail;
                }
                None => {
                    self.pending = Some(hi);
                    return;
                }
            }
        }
        let (pairs, tail) = rest.as_chunks::<2>();
        for pair in pairs {
            self.sum += u32::from(u16::from_be_bytes(*pair));
        }
        if let [last] = tail {
            self.pending = Some(*last);
        }
    }

    /// The complemented, folded sum. A buffer that includes its own correct
    /// checksum finishes to zero.
    #[must_use]
    pub fn finish(self) -> u16 {
        let mut sum = self.sum;
        if let Some(hi) = self.pending {
            sum += u32::from(hi) << 8;
        }
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }
}

/// The Internet checksum of `bytes`.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u16 {
    let mut c = Checksum::new();
    c.add(bytes);
    c.finish()
}

/// The pseudo-header that UDP and TCP checksums cover: source and destination
/// addresses, the protocol number, and the segment length.
#[must_use]
pub fn pseudo_header(src: [u8; 4], dst: [u8; 4], protocol: u8, length: u16) -> Checksum {
    let mut c = Checksum::new();
    c.add(&src);
    c.add(&dst);
    c.add(&[0, protocol]);
    c.add(&length.to_be_bytes());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc1071_example_sums_to_ddf2() {
        let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let mut c = Checksum::new();
        c.add(&data);
        // The folded sum is 0xddf2, and its complement is the checksum.
        assert_eq!(!c.finish(), 0xddf2);
        assert_eq!(checksum(&data), 0x220d);
    }

    #[test]
    fn odd_lengths_and_split_additions_agree() {
        let data = [0x12u8, 0x34, 0x56, 0x78, 0x9a];
        let whole = checksum(&data);
        for split in 0..=data.len() {
            let mut c = Checksum::new();
            c.add(&data[..split]);
            c.add(&data[split..]);
            assert_eq!(c.finish(), whole, "split at {split}");
        }
    }

    #[test]
    fn buffer_with_its_checksum_verifies_to_zero() {
        let mut data = [
            0x45u8, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0, 0, 10, 0, 0, 1,
        ];
        let ck = checksum(&data);
        data[10..12].copy_from_slice(&ck.to_be_bytes());
        assert_eq!(checksum(&data), 0);
    }
}
