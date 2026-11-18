# tpt-k-ipc-cap

The tpt-k IPC capabilities: tokens, rights, and unforgeable handles.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: rights, capability and handle types, and unforgeable tokens: an HMAC-SHA256 tag under a server key (SHA-256 from `tpt-primitives`), with verified attenuation. Deterministic property-style tests and bounded exhaustive tests cover attenuation, key binding and tamper detection.

Not done: the `CapabilityId` and CBOR types of `tpt-primitives` are not used yet. The crate holds no key and no table; the root server owns the key.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

