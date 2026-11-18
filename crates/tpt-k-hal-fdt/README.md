# tpt-k-hal-fdt

The tpt-k flattened device tree (FDT) parser.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: header checks, structure events, lookup of a property by path, root cell counts, memory regions and `reg` decoding. Read-only and allocation-free. Deterministic mutation tests cover the parser surface and `reg` decoding.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

