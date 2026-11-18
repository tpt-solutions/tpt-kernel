# tpt-k-drivers-synth

The tpt-k synth engine. It matches hardware IDs against known layouts. Micro-driver generation is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: PCI-style hardware-ID matching. A `Layout` rule sets any mix of vendor, device, class, subclass and programming-interface fields, and matches a device when every field it sets equals the device's. `match_best` picks the most specific rule, then the higher priority, then the first listed. A built-in table covers virtio-blk (legacy and modern), NVMe, xHCI and generic USB host controllers. Micro-driver generation is not implemented yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
