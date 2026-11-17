# tpt-k-boot-efi

UEFI ABI definitions and safe wrappers for the calls the tpt-k loader makes, written against UEFI 2.10.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 1, implemented. Only the table entries the loader calls have typed function pointers. Earlier entries are reserved slots of the correct size, so offsets match the specification. Entries after `Stall` are not declared, because the loader never reads past it.

The raw calls are `unsafe`. Each one states the firmware state it needs. The loader is the only caller, and it runs single-threaded before `ExitBootServices`.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

