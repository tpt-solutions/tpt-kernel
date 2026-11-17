# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- HID report descriptor parser into fixed-capacity input fields, with push/pop, report IDs, and long-item skipping.
- Boot keyboard report decoding with modifiers, roll-over detection, new-press diffing and a US usage-to-character table.
- Boot mouse report decoding (buttons and signed deltas).
