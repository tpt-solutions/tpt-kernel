# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Configuration-space enumeration over a caller-supplied `ConfigAccess`, and BAR decoding and sizing (32-bit and 64-bit memory, I/O). Decoding is switched off while BARs are sized. Bridges are not followed, and the caller supplies the ECAM access.
