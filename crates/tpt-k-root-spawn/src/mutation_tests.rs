// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. A fixed-seed
//! xorshift generator mutates a valid ELF64 image (byte flips, truncation,
//! header and program-header field overwrites, random bytes) and runs the
//! planner over the result. The only assertion is that nothing panics.

use super::*;
use std::vec::Vec;

const ITERATIONS: usize = 2000;

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

/// A valid image: one executable PT_LOAD and one read-write PT_LOAD.
fn sample() -> Vec<u8> {
    let mut b = std::vec![0u8; 0x3000];
    b[0..4].copy_from_slice(&ELF_MAGIC);
    b[4] = CLASS_64;
    b[5] = DATA_LSB;
    b[6] = 1;
    b[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
    b[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
    b[24..32].copy_from_slice(&0x40_1000u64.to_le_bytes());
    b[32..40].copy_from_slice(&(EHDR_LEN as u64).to_le_bytes());
    b[52..54].copy_from_slice(&(EHDR_LEN as u16).to_le_bytes());
    b[54..56].copy_from_slice(&(PHDR_LEN as u16).to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    let phs = [
        (PF_R | PF_X, 0x1000u64, 0x40_1000u64, 0x800u64, 0x800u64),
        (PF_R | PF_W, 0x2000u64, 0x40_2000u64, 0x100u64, 0x1000u64),
    ];
    for (i, (flags, offset, vaddr, filesz, memsz)) in phs.into_iter().enumerate() {
        let at = EHDR_LEN + i * PHDR_LEN;
        b[at..at + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[at + 4..at + 8].copy_from_slice(&flags.to_le_bytes());
        b[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        b[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        b[at + 24..at + 32].copy_from_slice(&vaddr.to_le_bytes());
        b[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
        b[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
        b[at + 48..at + 56].copy_from_slice(&PAGE_SIZE.to_le_bytes());
    }
    b
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    match rng.below(5) {
        0 => {
            for _ in 0..=rng.below(4) {
                let i = rng.below(v.len());
                v[i] ^= (rng.below(255) + 1) as u8;
            }
        }
        1 => {
            let n = rng.below(v.len() + 1);
            v.truncate(n);
        }
        2 => {
            // Overwrite a 64-bit field, where offsets, addresses and sizes live.
            let i = rng.below(v.len() - 7);
            v[i..i + 8].copy_from_slice(&rng.next().to_le_bytes());
        }
        3 => {
            let n = rng.below(4097);
            v = (0..n).map(|_| rng.next() as u8).collect();
        }
        _ => {
            // Put a page-scale value in a field, so alignment and overlap checks run.
            let i = rng.below(v.len() - 7);
            let value = (rng.below(16) as u64) * PAGE_SIZE;
            v[i..i + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    v
}

#[test]
fn mutation_elf_planner_never_panics() {
    let base = sample();
    assert!(plan(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0x7E57_E1F0_0000_0005);
    let mut planned = 0usize;
    for _ in 0..ITERATIONS {
        let v = mutate(&mut rng, &base);
        if let Ok(p) = plan(&v) {
            planned += 1;
            for seg in p.segments() {
                let _ = seg;
            }
        }
    }
    // Some mutations keep the image valid, so the segment walk runs.
    assert!(planned > 0);
}

#[test]
fn mutation_random_images_never_panic() {
    let mut rng = Rng(0x0BAD_E1F0_0000_0006);
    for _ in 0..ITERATIONS {
        let n = rng.below(1024);
        let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let _ = plan(&bytes);
    }
}
