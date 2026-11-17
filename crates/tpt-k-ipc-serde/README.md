# tpt-k-ipc-serde

The tpt-k IPC serialization: zero-copy (de)serialization and checksumming.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: Framed messages: a 16-byte header, 64-bit words and a payload, checked with CRC-32C. Words are read in place without copying. The checksum is scalar; the `tpt-simd` path is not used yet. Deterministic mutation tests cover the frame decoder, including repaired checksums.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

