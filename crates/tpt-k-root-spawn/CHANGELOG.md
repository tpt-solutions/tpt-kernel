# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Deterministic mutation tests cover the ELF planner.
- ELF64 x86-64 load planner: checks the header and program headers, and returns up to eight `PT_LOAD` segments (address, file range, memory size, R/W/X) and the entry point. Refuses writable+executable, overlapping, misaligned or out-of-image segments, and an entry outside an executable segment. It maps nothing; the caller does that from the plan.
