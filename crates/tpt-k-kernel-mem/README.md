# tpt-k-kernel-mem

Kernel memory for tpt-k: the boot handoff record and the physical page frame allocator.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Phase 2, implemented:

- `bootmap.rs`: the record the loader passes to the kernel, and the UEFI memory map it points at. The record is 80 bytes, little-endian, and the kernel accepts exactly one version (`VERSION`). The loader in `tpt-k-boot` mirrors the layout, so the two must agree.
- `lib.rs`: a physical frame allocator. It keeps a bitmap over one contiguous range of 4 KiB frames. The caller supplies the bitmap storage, so the allocator needs no heap.

- `clear_frame`: zeroes one 4 KiB frame with a scalar fill. `tpt-simd` acceleration is deferred.

Virtual memory and per-process address spaces are not done yet. Paging lives in `tpt-k-kernel-arch-x86_64`.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

