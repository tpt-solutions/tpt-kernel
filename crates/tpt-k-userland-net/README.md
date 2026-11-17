# tpt-k-userland-net

The tpt-k user-space network stack: Ethernet, ARP, IPv4, ICMP, UDP and TCP header codecs, and an ARP cache. The TCP state machine and the daemon are not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: pure protocol codecs with no sockets and no device I/O. Internet checksum and pseudo-header sums; Ethernet II; ARP (Ethernet/IPv4); IPv4 with fragment flags; ICMP echo; UDP; a TCP header parse and build without options (no state machine); and a fixed-capacity ARP cache with expiry.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

