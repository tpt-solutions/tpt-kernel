# tpt-k-root

The tpt-k root server: the first user-space process and the capability authority.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a capability service (`CapService`) that handles MINT, DELEGATE, REVOKE and ATTENUATE requests as `tpt-k-ipc` requests, checking signed tokens and rights first. The kernel handoff and the process start-up are not built yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

