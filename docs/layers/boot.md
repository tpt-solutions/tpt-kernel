# tpt-k-boot

The tpt-k UEFI bootloader: loads the kernel and root server images from the ESP.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Scaffold: crates are empty placeholders that build. Workspace members are under `crates/`.

## Build

This repo is bare-metal, so build it for its target (it has no host runtime):

```sh
cargo build --workspace --target x86_64-unknown-uefi
```

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE). See [LICENSE-POLICY.md](LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](CONTRIBUTING.md) for how to contribute.
