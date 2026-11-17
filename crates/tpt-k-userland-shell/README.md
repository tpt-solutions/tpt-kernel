# tpt-k-userland-shell

The tpt-k shell core: command parser and job table. Process spawning is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: a command-line parser (quoting, escapes, `|`, `&&`, `||`, `;`, `&`, `<`, `>`, `>>`) and a job table with `fg`/`bg` transitions, `%n` specs and reaping. Starting processes is not done yet; that belongs to the process layer.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

