# tpt-k-ipc-serde

The tpt-k IPC serialization: zero-copy (de)serialization and checksumming.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: framed messages with a 16-byte header, 64-bit words and a payload, checked with CRC-32C. Words are read in place without copying. Deterministic mutation tests cover the frame decoder, including repaired checksums, and deterministic property-style tests cover round trips, bit flips and arbitrary input.

Not done: the checksum is a scalar, table-driven CRC-32C; there is no SIMD path yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

