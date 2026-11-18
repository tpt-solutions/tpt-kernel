# tpt-k-hal-usb

The tpt-k USB HAL: a descriptor parser.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented and host-tested: device, configuration, interface and endpoint descriptor parsing, with HID boot keyboard and mouse recognition. Descriptors it does not know are returned unparsed for the class driver. Deterministic mutation tests cover the configuration and descriptor parsers.

Not done: the crate does not talk to a host controller or issue transfers; the caller supplies the descriptor bytes.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.

