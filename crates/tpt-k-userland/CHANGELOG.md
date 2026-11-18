# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Repl`: parses a line, runs the builtins `jobs`, `fg`, `bg`, `exit`, `true`, `false` and `echo`, applies `&&`, `||` and `;` by status, and records `&` jobs without running them. Output goes through a `Sink` trait.
- `Spawner` trait: every command that is not a builtin goes to it, through `run_line_with` and `run_reader_with`. `NoSpawner`, the default, reports `not supported yet` and returns status 127.
- `tpt-k-userland` binary: reads lines from standard input and exits with the `exit` code. It writes the `> ` prompt only when standard input is a terminal (`IsTerminal`), so piped transcripts contain only command output.
- Tests cover the builtins, short-circuiting, exit codes, job listing, a fake spawner and the binary over piped input.
