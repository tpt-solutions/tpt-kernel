// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. A fixed-seed
//! xorshift generator mutates a valid device tree (byte flips, truncation,
//! offset and length overwrites, random bytes) and the parser's public surface
//! is driven over the result. The only assertion is that nothing panics.

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
            // Overwrite a 32-bit word, which is where offsets and lengths live.
            if v.len() >= 4 {
                let i = rng.below(v.len() - 3);
                v[i..i + 4].copy_from_slice(&(rng.next() as u32).to_be_bytes());
            }
        }
        3 => {
            let n = rng.below(4097);
            v = (0..n).map(|_| rng.next() as u8).collect();
        }
        _ => {
            // Force a small value into a word, so offsets land inside the blob.
            if v.len() >= 4 {
                let i = rng.below(v.len() - 3);
                v[i..i + 4].copy_from_slice(&(rng.below(256) as u32).to_be_bytes());
            }
        }
    }
    v
}

fn cells(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// Builds a small but representative blob: root cells, memory, and a soc node.
fn sample() -> Vec<u8> {
    let mut structure: Vec<u8> = Vec::new();
    let mut strings: Vec<u8> = Vec::new();
    let name_off = |name: &str, strings: &mut Vec<u8>| -> u32 {
        let off = strings.len() as u32;
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        off
    };
    let token = |s: &mut Vec<u8>, t: u32| s.extend_from_slice(&t.to_be_bytes());
    let begin = |s: &mut Vec<u8>, name: &str| {
        s.extend_from_slice(&1u32.to_be_bytes());
        s.extend_from_slice(name.as_bytes());
        s.push(0);
        while !s.len().is_multiple_of(4) {
            s.push(0);
        }
    };
    let prop = |s: &mut Vec<u8>, off: u32, value: &[u8]| {
        s.extend_from_slice(&3u32.to_be_bytes());
        s.extend_from_slice(&(value.len() as u32).to_be_bytes());
        s.extend_from_slice(&off.to_be_bytes());
        s.extend_from_slice(value);
        while !s.len().is_multiple_of(4) {
            s.push(0);
        }
    };

    let ac = name_off("#address-cells", &mut strings);
    let sc = name_off("#size-cells", &mut strings);
    let reg = name_off("reg", &mut strings);
    let compat = name_off("compatible", &mut strings);
    let dtype = name_off("device_type", &mut strings);

    begin(&mut structure, "");
    prop(&mut structure, ac, &cells(&[2]));
    prop(&mut structure, sc, &cells(&[1]));
    begin(&mut structure, "memory@80000000");
    prop(&mut structure, dtype, b"memory\0");
    prop(&mut structure, reg, &cells(&[0, 0x8000_0000, 0x4000_0000]));
    token(&mut structure, 2);
    begin(&mut structure, "soc");
    begin(&mut structure, "uart@9000000");
    prop(&mut structure, compat, b"arm,pl011\0");
    token(&mut structure, 2);
    token(&mut structure, 2);
    token(&mut structure, 2);
    token(&mut structure, 9);

    let off_struct = 40 + 16;
    let off_strings = off_struct + structure.len();
    let total = off_strings + strings.len();
    let header = [
        0xD00D_FEEDu32,
        total as u32,
        off_struct as u32,
        off_strings as u32,
        40,
        17,
        16,
        0,
        strings.len() as u32,
        structure.len() as u32,
    ];
    let mut blob = Vec::new();
    for w in header {
        blob.extend_from_slice(&w.to_be_bytes());
    }
    blob.extend_from_slice(&[0; 16]);
    blob.extend_from_slice(&structure);
    blob.extend_from_slice(&strings);
    blob
}

#[test]
fn mutation_fdt_surface_never_panics() {
    let base = sample();
    assert!(Fdt::parse(&base).is_ok(), "sample must be valid");
    let mut rng = Rng(0xA5A5_1234_9876_0001);
    let mut parsed = 0usize;
    let paths = [
        "/",
        "/memory",
        "/memory@80000000",
        "/soc/uart",
        "/soc",
        "/nope",
        "",
    ];
    let names = [
        "#address-cells",
        "#size-cells",
        "reg",
        "compatible",
        "device_type",
        "",
    ];
    for _ in 0..ITERATIONS {
        let v = mutate(&mut rng, &base);
        let Ok(fdt) = Fdt::parse(&v) else {
            continue;
        };
        parsed += 1;
        for event in fdt.events().take(4096) {
            let _ = event;
        }
        let path = paths[rng.below(paths.len())];
        let name = names[rng.below(names.len())];
        let _ = fdt.property(path, name);
        let _ = fdt.root_cells();
        fdt.memory_regions(|base, size| {
            let _ = base.wrapping_add(size);
        });
    }
    // Some mutations must still parse, so the deeper walk is exercised.
    assert!(parsed > 0);
}

#[test]
fn mutation_reg_iter_never_panics() {
    let mut rng = Rng(0x0F0F_F0F0_1357_9BDF);
    for _ in 0..ITERATIONS {
        let n = rng.below(65);
        let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let addr = rng.below(5);
        let size = rng.below(5);
        for pair in RegIter::new(&bytes, addr, size).take(64) {
            let _ = pair;
        }
    }
}
