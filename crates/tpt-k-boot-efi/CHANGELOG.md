# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `unsafe` helpers (`find_configuration_table`, `locate_handles`, `allocate_pages`, `get_memory_map`) document why each firmware call is sound.
- UEFI 2.10 system table layout with reserved slots, so offsets match the specification.
- Typed function pointers for the boot and runtime service calls the loader needs.
