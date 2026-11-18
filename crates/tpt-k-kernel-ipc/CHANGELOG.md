# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Capability table with generation-checked handles, rights derivation, and recursive revocation.
- `delete_across`: deletes a capability without revoking anything else, and re-links its derived capabilities, in its table and across tables, to its own parent or origin. Host-tested.
- `CapTable::parent_at`, `CapTable::origin_at` and `Rights::bits`, for the capability inspector.
- Synchronous endpoints with bounded, heap-free FIFO queues.
- Endpoint messages can carry one capability (`send_transfer`, `take_delivery_transfer`).
- Bounded exhaustive host tests of the capability-table and endpoint invariants.
- Ignored host timing tests (`bench_tests.rs`) for endpoint send and receive, and for capability mint, derive and revoke.
