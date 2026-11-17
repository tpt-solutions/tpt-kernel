# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- UEFI loader that reads `TPTK\KERNEL.ELF` from the boot volume.
- Places the kernel's `PT_LOAD` segments at their physical addresses, exits boot services and jumps to the entry point.
