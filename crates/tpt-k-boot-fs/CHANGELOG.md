# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- ELF64 parser for little-endian x86_64 executables, reporting `PT_LOAD` segments.
- Bounds-checked reads: every offset is checked against the input length.
- Read-only FAT32 driver (`fat32`): boot sector checks, cluster-chain walking with loop detection, 8.3 lookup, and whole-file reads through a `BlockRead` trait.
- Long file name lookup in `fat32`: LFN chains are decoded into a fixed buffer, matched case-insensitively for ASCII names, and used only when complete and when their checksum matches the short entry. A bad or orphan chain falls back to the 8.3 name.
