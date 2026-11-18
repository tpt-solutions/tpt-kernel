# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Planner: layer lookup from crate name prefixes, targets per layer (kernel: `x86_64-unknown-none`, boot: `x86_64-unknown-uefi`, others: host), and deterministic cargo build or check commands.
- `plan` and `check`: print those commands for crates from `--fixture` or from `cargo metadata --no-deps` output (via `serde_json`) on a file or stdin. They never run cargo.
- `image`: prints the boot image steps. `image --run` builds the kernel and loader, and stages the ESP under `target/tpt-k/esp`.
- `run`: does `image --run`, then boots the image in QEMU with serial on the terminal.
- `assure`: the assurance gate over `docs/assurance/graph.json`. A claim needs evidence (`test`, `property`, `fuzz`, `exhaustive` or `proof`), or a waiver if it is critical. Each claim needs an `INV <id>:` line in `spec.txt`, and each such line must name a claim. A `proof` item needs a recorded `<property>: unsat` line in its `.log` file; the gate reads that record and does not run the solver. `--report <file>` writes the Markdown report, and `--html <file>` writes the dashboard as one static page.
- `doctor`: checks the Rust toolchain (pinned 1.95.0), the targets, QEMU, OVMF, the sibling repos and `core.hooksPath`, and prints `ok`, `warn` or `FAIL` for each with the fix commands for Windows or Linux. Exits 1 if a required check fails. The report logic is in `doctor.rs`, and is host-tested.
- `caps --serial <file|-> [--format text|dot]`: the capability inspector. It renders the kernel's `cap:` lines.
- `lint-manifest --manifest <file>`: compiles and lints a policy manifest (`tpt-k-root-manifest`).
- `replay --record <file>` and `replay --golden <file>`: boot on the QEMU instruction counter, and record or compare the serial transcript.

### Fixed

- `replay` is repeatable. The kernel's `replay` feature uses a fixed APIC timer count, and QEMU runs with a fixed instruction-counter shift, so the transcript is the same on every run.
