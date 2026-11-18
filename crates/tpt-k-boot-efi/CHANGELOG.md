# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- UEFI 2.10 system table layout with reserved slots, so offsets match the specification.
- Typed function pointers for the boot-service, text-output, file-system and file calls the loader makes. Runtime services are not declared.
- Lookups for firmware facts: `find_configuration_table` (for the ACPI 2.0 RSDP), `handle_protocol`, `locate_handles`, and the Graphics Output Protocol types.
- `Console` for ASCII text output, and `encode_path` for UTF-16 file paths.
- The `unsafe` helpers document the firmware state each call needs, and why the call is sound.
