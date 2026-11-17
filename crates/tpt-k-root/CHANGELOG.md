# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `CapService`: the root's capability service over `tpt-k-ipc`. MINT, DELEGATE, REVOKE and ATTENUATE check signed tokens and rights before touching the table. Host-tested.
- `main.rs` is still a placeholder with no kernel handoff.
