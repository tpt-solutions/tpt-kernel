# tpt-k-drivers-nvme

The tpt-k NVMe driver core: command and completion encodings, queue and doorbell math, and PRP planning. Controller I/O is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested, with no device I/O: 64-byte submission entries (Identify, Create I/O CQ and SQ, Read, Write, Flush), 16-byte completion decoding with the phase tag, submission and completion ring math, doorbell offsets for any `CAP.DSTRD`, and PRP planning for transfers of one or two pages or a list. The driver that drives the controller is not written yet.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

