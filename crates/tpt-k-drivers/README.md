# tpt-k-drivers

The tpt-k driver manager. It holds the restart policy for user-space drivers: heartbeats, backoff after a crash, and giving up after too many crashes in a row. It is a deterministic state machine. It does not spawn or kill processes; the caller carries out its actions.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a fixed-capacity registry of drivers, with restart on crash after exponential backoff, a kill for a missed heartbeat, a give-up after too many consecutive crashes, and stop requests that prevent restart. The process spawning that would carry out its actions is not built yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

