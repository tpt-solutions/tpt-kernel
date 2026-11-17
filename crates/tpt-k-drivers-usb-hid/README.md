# tpt-k-drivers-usb-hid

The tpt-k USB HID core: report descriptor parsing and boot keyboard and mouse report decoding. Device I/O is not built yet.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested, `no_std`: the HID report descriptor parser (fixed storage for input fields), the 8-byte boot keyboard report with roll-over handling and key presses, a US-layout usage-to-character table, and the 3-byte boot mouse report. The driver does not yet talk to a host controller.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

