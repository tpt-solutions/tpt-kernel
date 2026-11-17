# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Early boot entry: disables interrupts, switches to a kernel stack, and calls `kmain`.
- Phase 2 demo: preemptive workers, a kernel server, ring-3 threads over capability handles, and two isolated user address spaces.
- Panic handler and linker script for the x86_64 kernel image.
