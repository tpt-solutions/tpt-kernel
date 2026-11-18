# tpt-k-boot

The tpt-k UEFI bootloader: loads the kernel image from the ESP and hands over the boot information. Loading the root server image is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

What is built, and what is still open, is in [the layer checklist](../todos/boot.md). Each crate's README has its own `Status` section. Crates are under `crates/`.

## Build

The loader is a UEFI application, so build it for its target:

```sh
cargo build -p tpt-k-boot --target x86_64-unknown-uefi
```

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
