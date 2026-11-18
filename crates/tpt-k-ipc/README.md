# tpt-k-ipc

The tpt-k user-space IPC client and server framework.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: a request framework in which seven-word requests carry a capability token and `serve` checks the frame, the token, the opcode and the rights before the service runs. Every refusal still gets a reply. The `block` module defines the block-device protocol (read, write, flush, get-info), and `serve_block` checks requests against the device's sector count before the device runs.

Not done: there is no transport yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

