# tpt-k-userland

The tpt-k userland meta-crate and interactive shell binary.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

The shell REPL is built: it parses lines with `tpt-k-userland-shell`, runs the builtins `jobs`, `fg`, `bg`, `exit`, `true`, `false` and `echo`, and reads a script from standard input. External commands are reported as not supported yet, with status 127. Pipelines and redirections are refused with an error. Starting programs needs the process layer, which is not built.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

