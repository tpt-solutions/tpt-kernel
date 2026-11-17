# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Deterministic mutation tests cover the configuration and descriptor parsers.
- Device, configuration, interface and endpoint descriptor parser, with HID boot keyboard and mouse recognition. Descriptors it does not know are returned unparsed for the class driver.
