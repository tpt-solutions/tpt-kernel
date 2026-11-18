# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- HID report descriptor parser into fixed-capacity input fields (`ReportMap`), with push/pop, report IDs, and long-item skipping.
- Boot keyboard report decoding (`KeyboardReport`) with modifiers, roll-over detection, new-press diffing (`new_presses`) and a US usage-to-character table (`usage_to_char`).
- Boot mouse report decoding (`MouseReport`): buttons and signed deltas.
