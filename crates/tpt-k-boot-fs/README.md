# tpt-k-boot-fs

Boot-time image readers for tpt-k. Phase 1 has an ELF64 parser for the kernel image.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 1, implemented: ELF64 parsing (`src/elf.rs`) for little-endian x86_64 executables with `PT_LOAD` segments. Every offset is checked against the input length before it is read.

No FAT32 driver is needed yet. The loader reads the EFI System Partition through the UEFI file protocol.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

