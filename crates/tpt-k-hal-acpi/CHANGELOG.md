# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- RSDP and table-list parsing (RSDT and XSDT), the MADT (local APICs, I/O APICs, interrupt overrides, NMIs, address overrides and x2APIC) and the MCFG ECAM windows. Tables are checked for length and checksum; the MADT and MCFG parsers also check their signature.
- AML device discovery (`aml` module): walks DSDT and SSDT bodies for each device's `_HID` and `_ADR`. Control methods are skipped by length and never evaluated, and terms the walker cannot skip stop the walk with an error.
- Deterministic mutation tests cover the RSDP, system table, MADT and MCFG parsers. The iteration count can be raised with `TPT_FUZZ_ITERATIONS`.
