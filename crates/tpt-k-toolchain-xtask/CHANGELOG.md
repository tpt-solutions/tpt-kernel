# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `cargo xtask` with the tasks `check`, `build-kernel`, `qemu-smoke` and `fetch-deps`, in Rust, so they run on Windows without bash. The `xtask` alias is in `.cargo/config.toml`.
- The expected serial lines and the counts of the QEMU smoke test, moved here from `scripts/qemu-smoke.sh`, with unit tests on a sample transcript.
- The scripts in `scripts/` are thin wrappers that call these tasks.

### Fixed

- `qemu-smoke` boots the ESP it staged under its output directory, not the `target/tpt-k/esp` that `tpt-k-build image` stages.
- `qemu-smoke` reads the serial transcript from QEMU's stdout, not from a file.
- The license gate in `check` accepts `MIT-0` and `BSL-1.0`.
