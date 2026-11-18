# tpt-k-userland-fs-bridge

The tpt-k filesystem bridge core. It reads a file's bytes from an IPC block device through a write-through sector cache. It does not depend on tpt-filesystem yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a byte-range to sector-span mapper, a 16-sector write-through LRU cache, sector reads and writes through `BlockFs`, and `BlockFs::read_bytes`, which reads a `ByteFile` through the cache and returns short counts at end of file. Open: no tpt-filesystem integration, and no byte-level writes to files.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

