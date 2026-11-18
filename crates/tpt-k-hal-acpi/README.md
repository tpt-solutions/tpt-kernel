# tpt-k-hal-acpi

The tpt-k ACPI parser. It covers RSDP, MADT and MCFG, and walks DSDT and SSDT AML for device discovery only: `_HID` and `_ADR`. Control methods are not evaluated.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: RSDP and table-list parsing, the MADT (local APICs, I/O APICs, interrupt overrides, NMIs, x2APIC) and the MCFG ECAM windows. Tables are checked for length and checksum; the MADT and MCFG parsers also check their signature. DSDT and SSDT AML is walked for device discovery only (`_HID`, `_ADR`). Control methods are skipped, not evaluated, and terms the walker cannot skip are refused.

Not done: AML is not interpreted (no method evaluation, operation regions or fields). Deterministic mutation tests cover the RSDP, system table, MADT and MCFG parsers, not the AML walker.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

