# tpt-k-kernel

The tpt-k microkernel core: the trusted computing base (scheduling, physical memory, IPC, capability tables, interrupt routing).

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

What is built, and what is still open, is in [the layer checklist](../todos/kernel.md). Each crate's README has its own `Status` section. Crates are under `crates/`.

## Build

The kernel is bare-metal and has no host runtime. Build it with the build script, which passes the linker script:

```sh
scripts/build-kernel.sh
```

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
