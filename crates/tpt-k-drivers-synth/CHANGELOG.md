# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `HardwareId` and `Layout` types. A layout matches a device when every field it sets equals the device's: an exact vendor and device pair, class codes, or any mix.
- `match_best` precedence: exact vendor and device first, then more set fields, then priority, then listed order.
- `BUILTIN` table: virtio-blk (legacy and modern), NVMe, xHCI, and generic USB host controllers.
