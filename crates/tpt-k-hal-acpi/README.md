# tpt-k-hal-acpi

The tpt-k ACPI parser. It covers RSDP, MADT and MCFG, and walks DSDT and SSDT AML for device discovery only: `_HID` and `_ADR`. Control methods are not evaluated.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: RSDP and table-list parsing, the MADT (local APICs, I/O APICs, interrupt overrides, x2APIC) and the MCFG ECAM windows. Every table is checked for its signature, length and checksum. DSDT and SSDT AML is walked for device discovery only (`_HID`, `_ADR`). Control methods are skipped, not evaluated, and unknown terms are refused. Deterministic mutation tests cover the RSDP, system table, MADT and MCFG parsers.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

