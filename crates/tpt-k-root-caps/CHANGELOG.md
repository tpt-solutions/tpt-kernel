# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Deterministic property-style tests (seeded xorshift64, no external crates): over random mint, delegate and revoke sequences, live capabilities never hold rights their parent lacks, and revocation leaves no live descendant.
- Fixed-table capability slots for the root server: `mint`, `delegate` (rights may only narrow, and the caller needs GRANT), and `revoke` (needs REVOKE; kills the capability and all its descendants). Slots carry generation counters, so a handle to a revoked capability stays stale after its slot is reused. No heap.
