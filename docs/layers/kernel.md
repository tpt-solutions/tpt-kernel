# tpt-k-kernel

The tpt-k microkernel core: the trusted computing base (scheduling, physical memory, zero-copy IPC, interrupt routing).

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Scaffold: crates are empty placeholders that build. Workspace members are under `crates/`.

## Build

This repo is bare-metal, so build it for its target (it has no host runtime):

```sh
cargo build --workspace --target x86_64-unknown-none
```

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE). See [LICENSE-POLICY.md](LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](CONTRIBUTING.md) for how to contribute.
