# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- AML device discovery (`aml` module): walks DSDT and SSDT bodies for each device's `_HID` and `_ADR`. Control methods are skipped by length and never evaluated, and unknown terms stop the walk with an error. Host-tested, including a deterministic mutation test.
- Deterministic mutation tests cover the RSDP, system table, MADT and MCFG parsers.
- RSDP and table-list parsing, the MADT (local APICs, I/O APICs, interrupt overrides, x2APIC) and the MCFG ECAM windows. Every table is checked for its signature, length and checksum. DSDT and SSDT AML is not parsed yet, and there is no fuzzing yet.
