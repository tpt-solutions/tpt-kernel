# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- ELF64 parser for little-endian x86_64 executables, reporting `PT_LOAD` segments.
- Bounds-checked reads: every offset is checked against the input length.
