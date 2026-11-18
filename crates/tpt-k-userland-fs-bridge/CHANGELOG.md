# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Byte-range to sector-span mapper with overflow-checked arithmetic (`range`).
- Write-through LRU sector cache of 16 sectors, stored inline (`cache`), with `invalidate`.
- `BlockFs`: wraps an IPC block device, refuses devices whose sector size is not 512 bytes, and gives sector `read_sector`, `write_sector` and `flush` through the cache.
- `ByteFile` trait and `BlockFs::read_bytes`, which reads a file through the cache and returns short counts at end of file.
- Host tests over an in-memory block device.
