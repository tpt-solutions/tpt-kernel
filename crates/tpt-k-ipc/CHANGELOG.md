# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Client/server framework: seven-word requests that carry a capability token, and status replies with an optional payload. `serve` checks the frame, the token, the opcode and the rights before the service's `handle` runs, and every refusal still gets a reply. There is no transport yet.
- Block-device protocol (`block` module): read, write, flush and get-info requests as frames, status replies with a clear error set (out of range, I/O, read-only, bad request), a `BlockDevice` trait, and `serve_block`, which checks ranges and arithmetic before the device runs.
- Bounded exhaustive test of the service gate in `serve` (test-only).
