# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `CapService`: the root's capability service over `tpt-k-ipc`. The MINT, DELEGATE, REVOKE and ATTENUATE opcodes check the presented signed token and the rights each one needs before touching the table. `issue_root` hands out the first token. Host-tested.
- `main.rs` is still a placeholder with no kernel handoff.
