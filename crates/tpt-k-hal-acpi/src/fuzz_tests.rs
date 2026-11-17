// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. A fixed-seed
//! xorshift generator mutates valid tables (byte flips, truncation, length
//! field overwrites, random bytes, and checksum repair so that deeper code runs).
//! The only assertion is that no parser panics.

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
            if v.len() >= 4 {
                let i = rng.below(v.len() - 3);
                v[i..i + 4].copy_from_slice(&(rng.next() as u32).to_le_bytes());
            }
        }
        3 => {
            let n = rng.below(4097);
            v = (0..n).map(|_| rng.next() as u8).collect();
        }
        _ => {
            if v.len() >= 2 {
                let i = rng.below(v.len() - 1);
                v[i..i + 2].copy_from_slice(&(rng.next() as u16).to_le_bytes());
            }
        }
    }
    v
}

/// Recomputes an SDT checksum over the length the header claims.
fn refix_sdt(v: &mut [u8]) {
    if v.len() < SDT_HEADER_LEN {
        return;
    }
    let len = u32::from_le_bytes([v[4], v[5], v[6], v[7]]) as usize;
    if len < SDT_HEADER_LEN || len > v.len() {
        return;
    }
    v[9] = 0;
    let sum = v[..len].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    v[9] = 0u8.wrapping_sub(sum);
}

/// Recomputes the RSDP checksums (first 20 bytes, and the extended one).
fn refix_rsdp(v: &mut [u8]) {
    if v.len() < 20 {
        return;
    }
    v[8] = 0;
    let sum = v[..20].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    v[8] = 0u8.wrapping_sub(sum);
    if v[15] < 2 || v.len() < 36 {
        return;
    }
    let len = u32::from_le_bytes([v[20], v[21], v[22], v[23]]) as usize;
    if !(36..=v.len()).contains(&len) {
        return;
    }
    v[32] = 0;
    let sum = v[..len].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    v[32] = 0u8.wrapping_sub(sum);
}

fn sdt(signature: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(signature);
    t.extend_from_slice(&((SDT_HEADER_LEN + body.len()) as u32).to_le_bytes());
    t.push(1);
    t.push(0);
    t.extend_from_slice(b"TPTK  TESTTBL ");
    t.extend_from_slice(&[0u8; 12]);
    assert_eq!(t.len(), SDT_HEADER_LEN);
    t.extend_from_slice(body);
    refix_sdt(&mut t);
    t
}

fn madt_sample() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0xFEE0_0000u32.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&[0, 8, 0, 0, 1, 0, 0, 0]);
    body.extend_from_slice(&[1, 12, 2, 0]);
    body.extend_from_slice(&0xFEC0_0000u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&[2, 10, 0, 0]);
    body.extend_from_slice(&2u32.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&[4, 6, 0xFF, 0, 0, 1]);
    body.extend_from_slice(&[9, 16, 0, 0]);
    body.extend_from_slice(&0x20u32.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&3u32.to_le_bytes());
    body.extend_from_slice(&[5, 12, 0, 0]);
    body.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
    body.extend_from_slice(&[0x7F, 4, 0, 0]);
    sdt(b"APIC", &body)
}

fn mcfg_sample() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0u8; 8]);
    body.extend_from_slice(&0xE000_0000u64.to_le_bytes());
    body.extend_from_slice(&[0, 0, 0, 0xFF, 0, 0, 0, 0]);
    sdt(b"MCFG", &body)
}

fn xsdt_sample() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
    body.extend_from_slice(&0xFEE0_1000u64.to_le_bytes());
    sdt(b"XSDT", &body)
}

fn rsdt_sample() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x7000u32.to_le_bytes());
    body.extend_from_slice(&0x8000u32.to_le_bytes());
    sdt(b"RSDT", &body)
}

fn rsdp_sample() -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(b"RSD PTR ");
    r.push(0);
    r.extend_from_slice(b"TPTK  ");
    r.push(2);
    r.extend_from_slice(&0x1000u32.to_le_bytes());
    r.extend_from_slice(&36u32.to_le_bytes());
    r.extend_from_slice(&0x1234_5000u64.to_le_bytes());
    r.push(0);
    r.extend_from_slice(&[0; 3]);
    refix_rsdp(&mut r);
    r
}

#[test]
fn fuzz_rsdp_never_panics() {
    let base = rsdp_sample();
    assert!(Rsdp::parse(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..ITERATIONS {
        let mut v = mutate(&mut rng, &base);
        if rng.below(2) == 0 {
            refix_rsdp(&mut v);
        }
        let _ = Rsdp::parse(&v);
    }
}

#[test]
fn fuzz_sdt_and_root_tables_never_panic() {
    let bases = [xsdt_sample(), rsdt_sample()];
    assert!(
        bases.iter().all(|b| Sdt::parse(b).is_ok()),
        "samples must be valid"
    );
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    for i in 0..ITERATIONS {
        let mut v = mutate(&mut rng, &bases[i % 2]);
        if rng.below(2) == 0 {
            refix_sdt(&mut v);
        }
        if let Ok(sdt) = Sdt::parse(&v) {
            let _ = sdt.revision();
            let _ = sdt.body().len();
            if let Ok(addresses) = table_addresses(&sdt) {
                let _ = addresses.count();
            }
        }
    }
}

#[test]
fn fuzz_madt_entries_never_panic() {
    let base = madt_sample();
    assert!(Madt::parse(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for _ in 0..ITERATIONS {
        let mut v = mutate(&mut rng, &base);
        if rng.below(2) == 0 {
            refix_sdt(&mut v);
        }
        if let Ok(madt) = Madt::parse(&v) {
            let _ = madt.local_apic_address();
            let _ = madt.flags();
            for entry in madt.entries() {
                let _ = entry;
            }
        }
    }
}

#[test]
fn fuzz_mcfg_entries_never_panic() {
    let base = mcfg_sample();
    assert!(Mcfg::parse(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0x5EED_5EED_5EED_5EED);
    for _ in 0..ITERATIONS {
        let mut v = mutate(&mut rng, &base);
        if rng.below(2) == 0 {
            refix_sdt(&mut v);
        }
        if let Ok(mcfg) = Mcfg::parse(&v) {
            for entry in mcfg.entries() {
                let _ = entry;
            }
        }
    }
}
