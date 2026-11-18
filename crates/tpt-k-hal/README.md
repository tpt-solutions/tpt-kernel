# tpt-k-hal

The tpt-k hardware abstraction layer core: common hardware data structures.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: the resource types (`MmioRegion`, port ranges and `IrqLine`, combined as `Resource`) and the collision rule in `Resource::conflicts_with`. The crate holds the types and the rules only; the root server's grant logic is not here.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

