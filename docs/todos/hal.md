# tpt-k-hal — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: hardware description parsers, written from scratch with no GPL parsers.
ACPI, PCI, FDT, and USB.

Rules: `#![no_std]`, 100% Rust, no C/C++ bindings. Every parser gets unit tests and
fuzz targets.

---

## Phase 3 — HAL

- [ ] `tpt-k-hal`: core hardware data structures
- [ ] `tpt-k-hal-acpi`: MADT, MCFG, DSDT/SSDT parsing
- [ ] `tpt-k-hal-pci`: PCIe config space enumeration + BAR parsing
- [ ] Unit and fuzz tests for each parser against real-world table dumps

### Verification
- [ ] Milestone M3 (HAL side)

## Phase 6 — AArch64

- [ ] `tpt-k-hal-fdt`: Flattened Device Tree parser

## Phase 8 — Driver expansion

- [ ] `tpt-k-hal-usb`: USB descriptor parser

## Phase 9 — Bare metal

- [ ] ACPI/PCI compatibility fixes from real machine dumps

## Assurance

- [ ] Fuzzing campaigns for the ACPI, PCI, FDT, and USB parsers (feeds Phase 5 in the master plan)
