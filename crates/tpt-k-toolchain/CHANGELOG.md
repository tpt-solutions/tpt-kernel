# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Planner: layer lookup from crate name prefixes, targets per layer (kernel: `x86_64-unknown-none`, boot: `x86_64-unknown-uefi`, others: host), and deterministic plans of cargo build or check commands.
- `tpt-k-build` CLI with `plan`, `check`, `image` and `help`. It prints commands and never runs cargo.
- Crate lists from `--fixture` or from `cargo metadata --no-deps` output (via `serde_json`) on a file or stdin.
