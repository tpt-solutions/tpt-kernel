// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. A fixed-seed
//! xorshift generator mutates valid frames (byte flips, truncation, length
//! overwrites, random bytes). Half the mutants get a repaired checksum, so the
//! decoder also runs past the integrity check. The only assertion is that
//! nothing panics.

use super::*;
use std::vec::Vec;

const ITERATIONS: usize = 2000;

/// The iteration count: `ITERATIONS`, or the value of `TPT_FUZZ_ITERATIONS` when it
/// is set, so a longer campaign runs the same seeded cases for more iterations.
fn iterations() -> usize {
    std::env::var("TPT_FUZZ_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(ITERATIONS)
}

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

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    match rng.below(5) {
        0 => {
            for _ in 0..=rng.below(4) {
                if !v.is_empty() {
                    let i = rng.below(v.len());
                    v[i] ^= (rng.below(255) + 1) as u8;
                }
            }
        }
        1 => {
            let n = rng.below(v.len() + 1);
            v.truncate(n);
        }
        2 => {
            // Word count and payload length live at offsets 6 and 8.
            if v.len() >= 12 {
                let i = [6usize, 8][rng.below(2)];
                v[i..i + 2].copy_from_slice(&(rng.next() as u16).to_le_bytes());
            }
        }
        3 => {
            let n = rng.below(4097);
            v = (0..n).map(|_| rng.next() as u8).collect();
        }
        _ => {
            if !v.is_empty() {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
        }
    }
    v
}

/// Recomputes the checksum of a frame whose header claims a size that fits.
fn repair_checksum(v: &mut [u8]) {
    if v.len() < HEADER_LEN {
        return;
    }
    let words = u16::from_le_bytes([v[6], v[7]]) as usize;
    let payload = u32::from_le_bytes([v[8], v[9], v[10], v[11]]) as usize;
    let total = frame_len(words, payload);
    if total > v.len() {
        return;
    }
    v[12..16].copy_from_slice(&[0; 4]);
    let crc = frame_checksum(&v[..total]);
    v[12..16].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn mutation_decode_and_frame_access_never_panic() {
    let mut buf = [0u8; 128];
    let n = encode(
        &mut buf,
        &[1, u64::MAX, 0x0123_4567_89AB_CDEF],
        b"hello, endpoint",
    )
    .unwrap();
    let base = buf[..n].to_vec();
    assert!(decode(&base).is_ok(), "sample must be valid");

    let mut rng = Rng(0xFACE_B00C_0000_0003);
    let mut accepted = 0usize;
    for _ in 0..iterations() {
        let mut v = mutate(&mut rng, &base);
        if rng.below(2) == 0 {
            repair_checksum(&mut v);
        }
        let Ok(frame) = decode(&v) else {
            continue;
        };
        accepted += 1;
        let _ = frame.word_count();
        let _ = frame.payload().len();
        let _ = frame.word(rng.below(8));
        let _ = frame.word(usize::MAX);
        for w in frame.words() {
            let _ = w;
        }
    }
    // Repaired checksums must sometimes produce an accepted frame.
    assert!(accepted > 0);
}

#[test]
fn mutation_random_inputs_never_panic() {
    let mut rng = Rng(0x0DDB_A110_0000_0004);
    for _ in 0..iterations() {
        let n = rng.below(300);
        let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let _ = decode(&bytes);
    }
}
