# tpt-k-drivers-virtio

The tpt-k VirtIO layers: the split virtqueue and virtio-blk request encodings. Device I/O and the network driver are not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: Split virtqueue (VirtIO 1.x) layout, descriptors, avail and used rings, a no-heap descriptor chain allocator, the virtio-blk request chain and config, and the device status sequence. No device I/O yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

