# tpt-k-root-caps

The tpt-k root capabilities: generation, delegation, and revocation.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: Fixed-table capability slots for the root server: `mint`, `delegate` (rights may only narrow, and the caller needs GRANT), and `revoke` (needs REVOKE; kills the capability and all its descendants). Slots carry generation counters, so a handle to a revoked capability stays stale after its slot is reused. No heap.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

