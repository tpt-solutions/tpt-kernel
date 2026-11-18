// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. A fixed-seed
//! xorshift generator mutates valid descriptors (byte flips, truncation,
//! length and total-length overwrites, random bytes) and drives each parser.
//! The only assertion is that no parser panics.

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
            // Length and total-length fields are small and little-endian.
            if !v.is_empty() {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
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

fn configuration_sample() -> Vec<u8> {
    let mut ifc = Vec::new();
    ifc.extend_from_slice(&[9, 4, 0, 0, 1, 3, 1, 1, 0]);
    ifc.extend_from_slice(&[9, 0x21, 0x11, 0x01, 0, 1, 0x22, 63, 0]);
    ifc.extend_from_slice(&[7, 5, 0x81, 0x03, 8, 0, 10]);
    let total = 9 + ifc.len();
    let mut c = Vec::new();
    c.extend_from_slice(&[9, 2, total as u8, 0, 1, 1, 0, 0xA0, 50]);
    c.extend_from_slice(&ifc);
    c
}

fn device_sample() -> Vec<u8> {
    std::vec![18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x6B, 0x1D, 0x02, 0x00, 0x00, 0x06, 1, 2, 3, 1]
}

#[test]
fn mutation_configuration_and_descriptors_never_panic() {
    let base = configuration_sample();
    assert!(Configuration::parse(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0x1111_2222_3333_4444);
    let mut parsed = 0usize;
    for _ in 0..iterations() {
        let v = mutate(&mut rng, &base);
        let Ok(config) = Configuration::parse(&v) else {
            continue;
        };
        parsed += 1;
        let _ = (config.value, config.num_interfaces, config.attributes);
        for item in config.descriptors() {
            match item {
                Ok(Descriptor::Interface(i)) => {
                    let _ = (i.is_hid_boot_keyboard(), i.is_hid_boot_mouse());
                }
                Ok(Descriptor::Endpoint(e)) => {
                    let _ = (e.number, e.max_packet_size, e.interval);
                }
                Ok(Descriptor::Other { bytes, .. }) => {
                    let _ = bytes.len();
                }
                Err(_) => break,
            }
        }
    }
    // Some mutations keep the header valid, so the descriptor walk runs.
    assert!(parsed > 0);
}

#[test]
fn mutation_device_and_leaf_descriptors_never_panic() {
    let base = device_sample();
    assert!(Device::parse(&base).is_ok(), "sample must be valid");
    let endpoint = [7u8, 5, 0x81, 0x03, 8, 0, 10];
    let interface = [9u8, 4, 0, 0, 1, 3, 1, 1, 0];
    assert!(Endpoint::parse(&endpoint).is_ok());
    assert!(Interface::parse(&interface).is_ok());
    let mut rng = Rng(0x5555_6666_7777_8888);
    for i in 0..iterations() {
        let v = match i % 3 {
            0 => mutate(&mut rng, &base),
            1 => mutate(&mut rng, &endpoint),
            _ => mutate(&mut rng, &interface),
        };
        let _ = Device::parse(&v);
        let _ = Endpoint::parse(&v);
        let _ = Interface::parse(&v);
    }
}
