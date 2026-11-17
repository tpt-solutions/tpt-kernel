# tpt-k-hal-pci

The tpt-k PCIe HAL: config space enumeration and BAR parsing.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: Configuration-space enumeration over a caller-supplied `ConfigAccess`, and BAR decoding and sizing (32-bit and 64-bit memory, I/O). Decoding is switched off while BARs are sized. Bridges are not followed, and the caller supplies the ECAM access. Deterministic mutation tests cover BAR decoding and sizing, enumeration and BAR probing.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

