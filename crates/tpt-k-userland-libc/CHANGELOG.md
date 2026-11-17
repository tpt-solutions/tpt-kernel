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
- errno values `EBUSY` (16) and `EDEADLK` (35), the kernel's failure codes for a busy object and a would-deadlock call.
- C-named exports `strlen`, `strnlen`, `strcmp`, `strncmp`, `memcpy`, `memset` and `memcmp` (`cabi`), behind the `c-abi` feature. They are off by default, because the names clash with a system libc. The copy and fill loops use volatile accesses, so LLVM cannot lower them back into a call to these exports.
