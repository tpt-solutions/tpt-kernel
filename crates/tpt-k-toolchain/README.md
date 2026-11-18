# tpt-k-toolchain

The tpt-k build CLI (`tpt-k-build`). It plans the cargo commands for the workspace, checks the assurance graph and the machine, builds and boots the boot image, and reads the kernel's capability dump and policy manifests.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested. The commands:

- `plan` and `check` print the cargo build or check commands for each crate, grouped by target. They never run cargo.
- `image` prints the boot image steps. `image --run` builds the kernel and loader, and stages the ESP under `target/tpt-k/esp`.
- `run` does `image --run`, then boots the image in QEMU with the serial port on the terminal.
- `replay --record <file>` and `replay --golden <file>` boot the kernel built with its `replay` feature on QEMU's instruction counter, so the serial transcript is the same on every run. `--record` writes it, and `--golden` compares it with a trace such as `replay/golden.txt`.
- `caps --serial <file|->` renders the kernel's `cap:` lines as a graph (`--format text` or `dot`).
- `lint-manifest --manifest <file>` parses, compiles and lints a policy manifest.
- `assure` checks the assurance graph (`docs/assurance/graph.json`) against the evidence and `spec.txt`. `--report <file>` writes the Markdown report, and `--html <file>` the dashboard. It runs no cargo.
- `doctor` checks the Rust toolchain (1.95.0), the targets, QEMU, OVMF, the sibling repos and the git hooks path, and prints the fix for each problem. Run it from the repo root.

Crate lists for `plan` and `check` come from `--fixture` or from `cargo metadata` output on a file or stdin. `run`, `image --run` and `replay` need QEMU and OVMF. Assembling the image as a GPT disk with `tpt-k-toolchain-iso` is not wired yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
