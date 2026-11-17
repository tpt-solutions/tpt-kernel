# tpt-k-ipc

The tpt-k user-space IPC client and server framework.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: Client/server framework: seven-word requests that carry a capability token, and one-word status replies. `serve` verifies the token and the rights before the service runs. There is no transport yet.

The `block` module defines the block-device protocol (read, write, flush, get-info) used by drivers and userland. Requests are checked against the device's sector count before the device runs.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

