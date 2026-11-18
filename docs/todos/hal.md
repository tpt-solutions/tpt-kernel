# tpt-k-hal — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: hardware description parsers, written from scratch with no GPL parsers.
ACPI, PCI, FDT, and USB.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings. Every parser gets unit tests and
fuzz targets.

---

## Phase 3 — HAL

- [x] `tpt-k-hal`: core hardware data structures (2026-11-17: MMIO regions, port ranges, IRQ lines and the conflict rule the root server checks; host-tested)
- [ ] `tpt-k-hal-acpi`: MADT, MCFG, DSDT/SSDT parsing (partly, 2026-11-17: RSDP, RSDT/XSDT, MADT and MCFG are parsed and host-tested; the AML walker finds devices with _HID and _ADR, skipping method bodies; no OperationRegion, method evaluation or real firmware tables yet)
- [x] `tpt-k-hal-pci`: PCIe config space enumeration + BAR parsing (2026-11-17: enumeration over a caller-supplied `ConfigAccess`; BAR decoding and sizing with decoding switched off; host-tested. Bridges are not followed)
- [ ] Unit and fuzz tests for each parser against real-world table dumps (partly, 2026-11-17: synthetic-table unit tests for every parser; fuzz targets and real dumps are open)

### Verification
- [ ] Milestone M3 (HAL side)

## Phase 6 — AArch64

- [x] `tpt-k-hal-fdt`: Flattened Device Tree parser (2026-11-17: header checks, structure events, path lookup, root cells, memory regions and `reg` decoding; host-tested)

## Phase 8 — Driver expansion

- [x] `tpt-k-hal-usb`: USB descriptor parser (2026-11-17: device, configuration, interface and endpoint descriptors, HID boot keyboard and mouse recognition; host-tested)

## Phase 9 — Bare metal

- [ ] ACPI/PCI compatibility fixes from real machine dumps

## Assurance

- [ ] Fuzzing campaigns for the ACPI, PCI, FDT, and USB parsers (feeds Phase 5 in the master plan) (partly, 2026-11-17: deterministic mutation tests with 2000 cases each, no panics found; not coverage-guided fuzzing)
  - 2026-11-19: seeded campaigns of one million iterations per parser pass, with fixed seeds (`TPT_FUZZ_ITERATIONS=1000000`). Logs are in `docs/assurance/campaigns/tpt-k-hal-{acpi,fdt,pci,usb}.txt` (commit `bc0a0a6`), and claim `FUZZ-1` cites them. Still open: a coverage-guided run, and real table dumps.

## Phase 11a — stub cleanup (2026-11-18)

- [x] `fuzz_tests.rs` renamed to `mutation_tests.rs` in `tpt-k-hal-acpi`, `-fdt`, `-pci` and `-usb`. These are deterministic mutation tests, not coverage-guided fuzzing; that stays open
