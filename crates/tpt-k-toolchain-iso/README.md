# tpt-k-toolchain-iso

The tpt-k boot image assembly. It builds a FAT32 boot partition in memory: the loader, kernel and root images in 8.3 names.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: `build` writes a FAT32 volume from `(path, bytes)` pairs. It writes the boot sector, FSInfo, a backup boot sector at 6, two FAT copies, directory clusters and file data. Every image is read back through `tpt-k-boot-fs` in the tests.

`gpt::wrap` puts a volume in a raw GPT disk image: a protective MBR, primary and backup GPT headers with CRC-32, and one EFI System Partition entry at LBA 2048. There is no ISO 9660 wrapper, because UEFI reads the GPT disk directly.

Sizing: volumes are padded up to the FAT32 minimum of 65,525 clusters. That is 34,089,472 bytes (about 32.5 MiB) with 512-byte clusters. Content above 256 MiB uses 4 KiB clusters. The volume is otherwise sized to the content.

Limits: names must be 8.3 (no long file names). A directory is one cluster, so at 512-byte clusters it holds 16 entries. Files are limited to 4 GiB - 1 byte. The `..` entry of a directory directly under the root holds cluster 0, as the FAT specification says. `tpt-k-boot-fs` does not follow that entry, so `..` out of such a directory fails in the reader.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

