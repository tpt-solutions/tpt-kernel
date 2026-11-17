# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `delete_across`: deletes a capability without revoking anything else, and re-links its derived capabilities, in its table and across tables, to its own parent or origin. Host-tested.
- `CapTable::parent_at` and `Rights::bits`, for the capability inspector.

- Capability table with generation-checked handles, rights derivation, and recursive revocation.
- Synchronous endpoints with bounded, heap-free FIFO queues.
