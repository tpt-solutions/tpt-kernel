// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deterministic mutation tests, not coverage-guided fuzzing. BAR decoding and
//! sizing run over fixed-seed random register values, and enumeration and BAR
//! probing run against a configuration space that returns random dwords. The
//! only assertion is that nothing panics.

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
}

/// Config space that answers every read with a hash of its address and a seed.
/// Writes are ignored, so sizing reads back the same random values.
struct RandomConfig {
    seed: u64,
}

impl ConfigAccess for RandomConfig {
    fn read32(&mut self, bdf: Bdf, offset: u16) -> u32 {
        let mut x = self
            .seed
            .wrapping_add(bdf.ecam_offset())
            .wrapping_add(offset as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 31;
        x as u32
    }

    fn write32(&mut self, _bdf: Bdf, _offset: u16, _value: u32) {}
}

#[test]
fn fuzz_bar_decode_and_size_never_panic() {
    let mut rng = Rng(0x0BAD_5EED_0000_0001);
    for _ in 0..ITERATIONS {
        let low = rng.next() as u32;
        let high = if rng.next() & 1 == 0 {
            Some(rng.next() as u32)
        } else {
            None
        };
        if let Some((kind, base, _prefetchable)) = decode_bar(low, high) {
            let _ = base.wrapping_add(size_from_probe(kind, low, high));
        }
        // Also exercise each kind directly, including the all-ones probe value.
        for kind in [BarKind::Memory32, BarKind::Memory64, BarKind::Io] {
            let _ = size_from_probe(kind, low, high);
            let _ = size_from_probe(kind, u32::MAX, Some(u32::MAX));
            let _ = size_from_probe(kind, 0, Some(0));
        }
    }
}

#[test]
fn fuzz_enumerate_and_probe_never_panic() {
    let mut rng = Rng(0x0C0F_FEE0_0000_0002);
    for _ in 0..ITERATIONS {
        let mut access = RandomConfig { seed: rng.next() };
        // Keep the bus range small so each iteration stays cheap in debug mode.
        let start = (rng.next() % 4) as u8;
        let end = start + (rng.next() % 2) as u8;
        let mut found: Vec<Function> = Vec::new();
        enumerate(&mut access, start, end, |f| found.push(f));
        for f in found {
            probe_bars(&mut access, f.bdf, f.header_type, |bar| {
                let _ = bar.base.checked_add(bar.size);
            });
        }
    }
}
