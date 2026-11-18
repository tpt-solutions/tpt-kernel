# tpt-k-boot-fs

Boot-time image readers for tpt-k: an ELF64 parser for the kernel image, and a read-only FAT32 driver.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested. ELF64 parsing (`src/elf.rs`) covers little-endian x86_64 `ET_EXEC` images with `PT_LOAD` segments. Every offset is checked against the input length before it is read.

The read-only FAT32 driver (`src/fat32.rs`) checks the boot sector and refuses volumes that are not FAT32. It walks cluster chains, looks up names case-insensitively by their 8.3 form or their ASCII long file name (LFN chains are checked against the short entry's checksum, and deleted entries are skipped), and reads files into a caller buffer. It needs no heap. Limits: long names must be ASCII and at most 255 bytes. The driver is not yet used by the loader, which still reads the EFI System Partition through the UEFI file protocol.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
