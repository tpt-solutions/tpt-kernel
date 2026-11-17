# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Split virtqueue (VirtIO 1.x) layout, descriptors, avail and used rings, a no-heap descriptor chain allocator, the virtio-blk request chain and config, and the device status sequence. No device I/O yet.
