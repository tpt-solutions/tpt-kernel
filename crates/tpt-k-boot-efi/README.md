# tpt-k-boot-efi

UEFI ABI definitions and safe wrappers for the calls the tpt-k loader makes, written against UEFI 2.10.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, and used by `tpt-k-boot`. It covers the system table, the boot-service calls the loader makes (memory, protocol lookup, `ExitBootServices`, `Stall`), text output, the file-system and file protocols, the Graphics Output Protocol, and the ACPI 2.0 RSDP lookup. Earlier boot-services entries are reserved slots of the correct size, so offsets match the specification. The boot-services table ends at `ConnectController`, and later entries are not declared.

Runtime services are not wrapped yet.

The raw calls are `unsafe`. Each one states the firmware state it needs. The loader is the only caller, and it runs single-threaded before `ExitBootServices`.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
