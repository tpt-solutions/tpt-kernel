# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Scaffold: placeholder crate that builds, with no functionality yet.
- `Repl`: parses a line, runs the builtins, applies `&&`, `||` and `;` by status, and records `&` jobs. Output goes through a `Sink` trait.
- `tpt-k-userland` binary: reads lines from standard input and exits with the `exit` code.
- Tests cover the builtins, short-circuiting, exit codes, job listing and the piped binary.
- `Spawner` trait: every command that is not a builtin goes to it, through `run_line_with` and `run_reader_with`. `NoSpawner`, the default, keeps the `not supported yet` result with status 127. Tests use a fake spawner.

### Changed

- The binary writes the `> ` prompt only when standard input is a terminal (`IsTerminal`). Piped transcripts no longer contain it.
