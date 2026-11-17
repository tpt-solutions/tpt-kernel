# tpt-k-boot-fs

Boot-time image readers for tpt-k: an ELF64 parser for the kernel image, and a read-only FAT32 driver.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 1, implemented: ELF64 parsing (`src/elf.rs`) for little-endian x86_64 executables with `PT_LOAD` segments. Every offset is checked against the input length before it is read.

Implemented, host-tested: a read-only FAT32 driver (`src/fat32.rs`). It checks the boot sector and refuses volumes that are not FAT32, walks cluster chains, looks up 8.3 names case-insensitively (skipping LFN and deleted entries), and reads files into a caller buffer. It needs no heap. Limits: long file names are not read, so a file is found only by its 8.3 name, and it is not yet used by the loader, which still reads the EFI System Partition through the UEFI file protocol.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

