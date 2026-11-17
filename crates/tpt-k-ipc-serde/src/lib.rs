// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k IPC serialization: a framed message format with a CRC-32C checksum.
//!
//! A frame is a 16-byte header, then `word_count` little-endian 64-bit words,
//! then a byte payload. The words are read in place as bytes, so a receiver
//! takes fields without copying or allocating. The checksum covers the whole
//! frame except its own field, so a corrupted or truncated frame is refused
//! before any field is used.
//!
//! Header layout (all little-endian):
//!
//! | offset | size | field                                |
//! |-------:|-----:|--------------------------------------|
//! | 0      | 4    | magic `TPTK`                         |
//! | 4      | 1    | version (1)                          |
//! | 5      | 1    | reserved, zero                       |
//! | 6      | 2    | word count                           |
//! | 8      | 4    | payload length in bytes              |
//! | 12     | 4    | CRC-32C of the frame, this field zero |

#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod mutation_tests;

/// Frame magic: the bytes `TPTK`.
pub const MAGIC: [u8; 4] = *b"TPTK";
/// The only frame version this crate reads or writes.
pub const VERSION: u8 = 1;
/// Length of the frame header.
pub const HEADER_LEN: usize = 16;

/// Why a frame could not be encoded or decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerdeError {
    /// The output buffer cannot hold the frame.
    BufferTooSmall,
    /// There are more than `u16::MAX` words.
    TooManyWords,
    /// The input ends before the header or the declared body does.
    Truncated,
    /// The magic bytes are not `TPTK`.
    BadMagic,
    /// The version is not one this crate reads.
    BadVersion,
    /// A reserved byte is not zero.
    BadReserved,
    /// The checksum does not match the frame.
    BadChecksum,
}

const CRC_TABLE: [u32; 256] = make_crc_table();

const fn make_crc_table() -> [u32; 256] {
    // Reflected Castagnoli polynomial.
    const POLY: u32 = 0x82F6_3B78;
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Incremental CRC-32C state. Start with [`Crc32c::new`], feed bytes with
/// [`Crc32c::update`], and read the result with [`Crc32c::finish`].
#[derive(Clone, Copy, Debug)]
pub struct Crc32c(u32);

impl Crc32c {
    /// A fresh checksum state.
    #[must_use]
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    /// Feeds `bytes` into the checksum.
    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for &b in bytes {
            crc = CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        self.0 = crc;
    }

    /// The checksum of everything fed so far.
    #[must_use]
    pub const fn finish(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32c {
    fn default() -> Self {
        Self::new()
    }
}

/// CRC-32C of `bytes`.
#[must_use]
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut c = Crc32c::new();
    c.update(bytes);
    c.finish()
}

/// Length of a frame with `words` words and `payload_len` payload bytes.
#[must_use]
pub const fn frame_len(words: usize, payload_len: usize) -> usize {
    HEADER_LEN + 8 * words + payload_len
}

/// Writes a frame into `out` and returns its length.
///
/// `words` are stored in order, then `payload` follows. The checksum is written
/// last, over the finished frame.
pub fn encode(out: &mut [u8], words: &[u64], payload: &[u8]) -> Result<usize, SerdeError> {
    let total = frame_len(words.len(), payload.len());
    let body = HEADER_LEN + 8 * words.len();
    let region = out.get_mut(..total).ok_or(SerdeError::BufferTooSmall)?;
    region[body..].copy_from_slice(payload);
    seal(region, words, payload.len())
}

/// Finishes a frame whose payload is already in place.
///
/// The payload must sit at `HEADER_LEN + 8 * words.len()` inside `out`, for
/// `payload_len` bytes. This writes the header and the words around it and then
/// the checksum, so a service can build its reply straight into the reply buffer.
pub fn seal(out: &mut [u8], words: &[u64], payload_len: usize) -> Result<usize, SerdeError> {
    let word_count = u16::try_from(words.len()).map_err(|_| SerdeError::TooManyWords)?;
    let len32 = u32::try_from(payload_len).map_err(|_| SerdeError::BufferTooSmall)?;
    let total = frame_len(words.len(), payload_len);
    let out = out.get_mut(..total).ok_or(SerdeError::BufferTooSmall)?;

    out[0..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = 0;
    out[6..8].copy_from_slice(&word_count.to_le_bytes());
    out[8..12].copy_from_slice(&len32.to_le_bytes());
    out[12..16].copy_from_slice(&[0; 4]);
    for (i, w) in words.iter().enumerate() {
        let at = HEADER_LEN + 8 * i;
        out[at..at + 8].copy_from_slice(&w.to_le_bytes());
    }

    let crc = frame_checksum(out);
    out[12..16].copy_from_slice(&crc.to_le_bytes());
    Ok(total)
}

/// Checksum of a frame with its checksum field taken as zero.
fn frame_checksum(frame: &[u8]) -> u32 {
    let mut c = Crc32c::new();
    c.update(&frame[..12]);
    c.update(&[0; 4]);
    c.update(&frame[16..]);
    c.finish()
}

/// A checked frame that borrows its bytes from the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    words: &'a [u8],
    payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Number of 64-bit words.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.words.len() / 8
    }

    /// Word `index`, or `None` past the end.
    #[must_use]
    pub fn word(&self, index: usize) -> Option<u64> {
        let at = index.checked_mul(8)?;
        self.words.get(at..at.checked_add(8)?)?;
        Some(read_word(self.words, at))
    }

    /// Iterates over the words in order.
    pub fn words(&self) -> impl Iterator<Item = u64> + 'a {
        let bytes = self.words;
        (0..self.word_count()).map(move |i| read_word(bytes, i * 8))
    }

    /// The payload bytes.
    #[must_use]
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

/// The little-endian word at byte offset `at`. The caller has checked that
/// eight bytes are present.
fn read_word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

/// Checks a frame and returns it. Only the first frame in `input` is read;
/// the rest of the slice is ignored.
pub fn decode(input: &[u8]) -> Result<Frame<'_>, SerdeError> {
    if input.len() < HEADER_LEN {
        return Err(SerdeError::Truncated);
    }
    if input[0..4] != MAGIC {
        return Err(SerdeError::BadMagic);
    }
    if input[4] != VERSION {
        return Err(SerdeError::BadVersion);
    }
    if input[5] != 0 {
        return Err(SerdeError::BadReserved);
    }
    let word_count = u16::from_le_bytes([input[6], input[7]]) as usize;
    let payload_len = u32::from_le_bytes([input[8], input[9], input[10], input[11]]) as usize;
    let total = frame_len(word_count, payload_len);
    let frame = input.get(..total).ok_or(SerdeError::Truncated)?;

    let stored = u32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]]);
    if frame_checksum(frame) != stored {
        return Err(SerdeError::BadChecksum);
    }
    let body = HEADER_LEN + 8 * word_count;
    Ok(Frame {
        words: &frame[HEADER_LEN..body],
        payload: &frame[body..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_matches_standard_check_value() {
        // The standard check value for CRC-32C.
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[]), 0);
        let mut split = Crc32c::new();
        split.update(b"1234");
        split.update(b"56789");
        assert_eq!(split.finish(), 0xE306_9283);
    }

    #[test]
    fn frame_round_trips_words_and_payload() {
        let mut buf = [0u8; 128];
        let words = [1u64, u64::MAX, 0x0123_4567_89AB_CDEF];
        let payload = b"hello, endpoint";
        let n = encode(&mut buf, &words, payload).unwrap();
        assert_eq!(n, frame_len(3, payload.len()));
        let f = decode(&buf[..n]).unwrap();
        assert_eq!(f.word_count(), 3);
        assert_eq!(f.word(1), Some(u64::MAX));
        assert_eq!(f.word(3), None);
        assert_eq!(f.words().collect::<std::vec::Vec<_>>(), words);
        assert_eq!(f.payload(), payload);
    }

    #[test]
    fn seal_matches_encode() {
        let words = [5u64, 6];
        let payload = b"in place";
        let mut a = [0u8; 64];
        let na = encode(&mut a, &words, payload).unwrap();
        let mut b = [0u8; 64];
        let body = frame_len(words.len(), 0);
        b[body..body + payload.len()].copy_from_slice(payload);
        let nb = seal(&mut b, &words, payload.len()).unwrap();
        assert_eq!(na, nb);
        assert_eq!(a[..na], b[..nb]);
    }

    #[test]
    fn empty_frame_round_trips() {
        let mut buf = [0u8; HEADER_LEN];
        let n = encode(&mut buf, &[], &[]).unwrap();
        assert_eq!(n, HEADER_LEN);
        let f = decode(&buf).unwrap();
        assert_eq!(f.word_count(), 0);
        assert!(f.payload().is_empty());
    }

    #[test]
    fn encode_reports_small_buffers_and_too_many_words() {
        let mut small = [0u8; 20];
        assert_eq!(
            encode(&mut small, &[1, 2], &[]),
            Err(SerdeError::BufferTooSmall)
        );
        let many = std::vec![0u64; u16::MAX as usize + 1];
        let mut big = std::vec![0u8; frame_len(many.len(), 0)];
        assert_eq!(encode(&mut big, &many, &[]), Err(SerdeError::TooManyWords));
    }

    #[test]
    fn decode_refuses_damage() {
        let mut buf = [0u8; 64];
        let n = encode(&mut buf, &[42], b"abc").unwrap();
        let good = buf[..n].to_vec();

        assert_eq!(decode(&good[..HEADER_LEN - 1]), Err(SerdeError::Truncated));
        assert_eq!(decode(&good[..n - 1]), Err(SerdeError::Truncated));

        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(decode(&bad), Err(SerdeError::BadMagic));

        let mut bad = good.clone();
        bad[4] = 2;
        assert_eq!(decode(&bad), Err(SerdeError::BadVersion));

        let mut bad = good.clone();
        bad[5] = 1;
        assert_eq!(decode(&bad), Err(SerdeError::BadReserved));

        let mut bad = good.clone();
        bad[HEADER_LEN] ^= 0x10;
        assert_eq!(decode(&bad), Err(SerdeError::BadChecksum));

        let mut bad = good.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert_eq!(decode(&bad), Err(SerdeError::BadChecksum));
    }

    #[test]
    fn every_single_byte_flip_is_detected() {
        let mut buf = [0u8; 64];
        let n = encode(&mut buf, &[7, 8], b"payload").unwrap();
        let good = buf[..n].to_vec();
        for i in 0..n {
            for bit in 0..8 {
                let mut bad = good.clone();
                bad[i] ^= 1 << bit;
                assert!(decode(&bad).is_err(), "flip at byte {i} bit {bit} accepted");
            }
        }
    }

    #[test]
    fn decode_never_panics_on_short_or_odd_inputs() {
        // Declared sizes far larger than the input must be refused, not read.
        let mut hostile = [0xFFu8; HEADER_LEN];
        hostile[0..4].copy_from_slice(&MAGIC);
        hostile[4] = VERSION;
        hostile[5] = 0;
        assert_eq!(decode(&hostile), Err(SerdeError::Truncated));
        for len in 0..HEADER_LEN {
            assert_eq!(decode(&hostile[..len]), Err(SerdeError::Truncated));
        }
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use std::vec::Vec;

    /// Deterministic xorshift64 generator. Seeded with a constant, so every run
    /// checks the same cases. This is property-style testing, not
    /// coverage-guided fuzzing.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// A value in `0..n`, for `n > 0`.
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn bytes(&mut self, len: usize) -> std::vec::Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    /// Cases per property. Enough to cover the input space in practice, and
    /// small enough that the suite stays fast in debug builds.
    const CASES: usize = 512;

    fn words(rng: &mut Rng) -> Vec<u64> {
        let n = rng.below(17);
        (0..n).map(|_| rng.next()).collect()
    }

    /// Invariant: framed messages round-trip exactly, so the IPC wire format
    /// loses no words or payload bytes.
    #[test]
    fn encode_then_decode_round_trips() {
        let mut rng = Rng(0xA076_1D64_78BD_642F);
        for _ in 0..CASES {
            let w = words(&mut rng);
            let len = rng.below(257);
            let payload = rng.bytes(len);
            let mut buf = std::vec![0u8; frame_len(w.len(), payload.len())];
            let n = encode(&mut buf, &w, &payload).unwrap();
            let f = decode(&buf[..n]).unwrap();
            assert_eq!(f.words().collect::<Vec<u64>>(), w);
            assert_eq!(f.payload(), &payload[..]);
        }
    }

    /// Invariant: corruption is detected before any field is used. Any single
    /// flipped bit of a frame makes decode fail.
    #[test]
    fn any_single_bit_flip_is_rejected() {
        let mut rng = Rng(0xE703_7ED1_A0B4_28DB);
        for _ in 0..CASES {
            let w = words(&mut rng);
            let len = rng.below(257);
            let payload = rng.bytes(len);
            let mut buf = std::vec![0u8; frame_len(w.len(), payload.len())];
            let n = encode(&mut buf, &w, &payload).unwrap();
            let pos = rng.below(n);
            let bit = rng.below(8) as u8;
            buf[pos] ^= 1 << bit;
            assert!(decode(&buf[..n]).is_err());
        }
    }

    /// Invariant: decoding untrusted bytes never panics, whatever the input.
    #[test]
    fn decode_never_panics_on_arbitrary_bytes() {
        let mut rng = Rng(0x8EBC_6AF0_9C88_C6E3);
        for _ in 0..CASES {
            let len = rng.below(513);
            let bytes = rng.bytes(len);
            if let Ok(f) = decode(&bytes) {
                let _ = f.words().count();
                let _ = f.word(0);
                let _ = f.payload().len();
            }
        }
    }
}
