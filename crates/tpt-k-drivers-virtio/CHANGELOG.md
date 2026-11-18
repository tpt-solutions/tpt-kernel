# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Split virtqueue (VirtIO 1.x) layout, with descriptor, avail and used ring encodings.
- No-heap descriptor chain allocator.
- virtio-blk request header, request chain and config decoding.
- Device status sequence. Host-tested; no device I/O.
