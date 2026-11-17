# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Boot handoff record (80 bytes, versioned) and the UEFI memory map it references.
- Physical page frame allocator over a bitmap supplied by the caller.
- `clear_frame`: zeroes one page, refusing buffers that are not exactly one frame. Scalar only; `tpt-simd` acceleration is deferred.
