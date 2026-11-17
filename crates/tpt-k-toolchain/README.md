# tpt-k-toolchain

The tpt-k build CLI (`tpt-k-build`). It resolves dependencies, cross-compiles workspaces, and links binaries.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a planner that groups workspace crates by layer and target and prints the cargo commands for them, plus the `tpt-k-build` CLI (`plan`, `check`, `image`). The CLI never runs cargo. Crate lists come from `--fixture` or from `cargo metadata` output on a file or stdin. Running the commands, and the ISO assembly, are not wired yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

