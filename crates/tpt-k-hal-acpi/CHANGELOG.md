# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- RSDP and table-list parsing, the MADT (local APICs, I/O APICs, interrupt overrides, x2APIC) and the MCFG ECAM windows. Every table is checked for its signature, length and checksum. DSDT and SSDT AML is not parsed yet, and there is no fuzzing yet.
