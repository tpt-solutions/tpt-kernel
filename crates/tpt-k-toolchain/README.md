# tpt-k-toolchain

The tpt-k build CLI (`tpt-k-build`). It resolves dependencies, cross-compiles workspaces, and links binaries.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a planner that groups workspace crates by layer and target and prints the cargo commands for them, plus the `tpt-k-build` CLI (`plan`, `check`, `image`, `image --run`, `run`, `caps`, `replay`, `lint-manifest`, `assure` and `assure --html`, `doctor`). `run` and `image --run` build the image and stage the ESP; `run` boots it in QEMU. `caps` renders the kernel's capability dump. `lint-manifest` checks a policy manifest. `replay --golden replay/golden.txt` checks a boot against the recorded trace, and `replay --record <file>` writes a new one. The kernel is built with its `replay` feature (a fixed APIC timer count instead of the PIT calibration), and QEMU runs with a fixed instruction-counter shift, so the transcript is the same on every run. Mirror of the commands: see the usage text. `doctor` probes the machine for the toolchain, targets, QEMU, OVMF, the sibling repos and the git hooks path, and prints the fix for each problem. Run it from the repo root. The other commands never run cargo. Crate lists come from `--fixture` or from `cargo metadata` output on a file or stdin. Running the commands, and the ISO assembly, are not wired yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

