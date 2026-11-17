# tpt-k-userland-libc

The tpt-k minimal POSIX shim over tpt-k-ipc.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: allocation-free memory and string functions over byte slices, `strtol`-style parsing, a `printf` subset for integers into a caller buffer, errno values, and path normalisation. No `extern "C"` surface, no `tpt-k-ipc` mapping and no kernel calls yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

