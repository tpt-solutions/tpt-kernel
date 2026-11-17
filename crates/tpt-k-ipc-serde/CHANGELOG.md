# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Deterministic property-style tests (seeded xorshift64, no external crates): frames round-trip any words and payload, any single flipped bit is rejected, and decode never panics on arbitrary bytes.
- Deterministic mutation tests cover the frame decoder, including repaired checksums.
- Framed messages: a 16-byte header, 64-bit words and a payload, checked with CRC-32C. Words are read in place without copying. The checksum is scalar; the `tpt-simd` path is not used yet.
