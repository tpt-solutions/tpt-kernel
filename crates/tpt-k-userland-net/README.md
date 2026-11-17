# tpt-k-userland-net

The tpt-k user-space network stack: Ethernet, ARP, IPv4, ICMP, UDP and TCP codecs, an ARP cache, and a TCP connection state machine. The daemon is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: pure protocol codecs with no sockets and no device I/O. Internet checksum and pseudo-header sums; Ethernet II; ARP (Ethernet/IPv4); IPv4 with fragment flags; ICMP echo; UDP; a TCP header parse and build without options; a TCP connection state machine (RFC 793, no retransmission, no congestion control, no options); and a fixed-capacity ARP cache with expiry.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

