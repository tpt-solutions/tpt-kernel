# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Internet checksum with incremental sums and pseudo-header sums (RFC 1071).
- Ethernet II header, ARP (Ethernet/IPv4), IPv4 with checksum and fragment flags, ICMP echo, UDP, and a TCP header parse and build without options.
- Fixed-capacity ARP cache with expiry and oldest-entry eviction.
