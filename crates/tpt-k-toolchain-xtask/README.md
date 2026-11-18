# tpt-k-toolchain-xtask

The repo's tasks, in Rust, so they run the same on Windows and Linux: `check`, `build-kernel`, `qemu-smoke` and `fetch-deps`. Run them from anywhere in the repo with `cargo xtask <task>`, which is an alias in `.cargo/config.toml`.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented:

- `check` runs fmt, clippy, the host tests, the assurance gate (`tpt-k-build assure`) and the license gate. The license gate runs `tpt-cargo-vitals`, and is skipped with a message if that is not on PATH.
- `build-kernel` builds the kernel with its linker script.
- `qemu-smoke` builds the kernel and the loader, stages the ESP, boots QEMU, and checks the exit status and the serial transcript against the expected lines and counts. Environment overrides: `QEMU`, `OVMF_CODE`, `OVMF_VARS`, `OUT` and `TIMEOUT`.
- `fetch-deps` clones or fast-forwards the sibling repos, and skips any repo with local changes.

The shell scripts in `scripts/` are thin wrappers that call this binary. The serial checks are unit-tested on a sample transcript, and the QEMU run itself is covered by `cargo xtask qemu-smoke`.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
