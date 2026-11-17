# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `cargo xtask` with the tasks `check`, `build-kernel`, `qemu-smoke` and `fetch-deps`, in Rust, so they run on Windows without bash.
- The expected serial lines and the counts of the QEMU smoke test, moved here from `scripts/qemu-smoke.sh`, with unit tests on a sample transcript.
