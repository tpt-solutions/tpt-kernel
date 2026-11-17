# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Memory and string functions over byte slices (`mem`, `string`).
- `strtol`-style integer parsing with overflow and no-digit errors (`num`).
- Integer formatting into a caller buffer for `%d`, `%u`, `%x` and `%o`, with width and zero padding (`fmt`).
- errno values and their numeric codes (`errno`).
- Path normalisation that refuses to climb above the root (`path`).
