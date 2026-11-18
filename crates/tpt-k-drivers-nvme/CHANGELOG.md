# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 64-byte submission and 16-byte completion entry encodings, with the phase tag on completions.
- Admin and I/O command builders for Identify, Create I/O CQ and SQ, Read, Write and Flush.
- Submission and completion ring math, doorbell offsets for any `CAP.DSTRD`, and PRP planning for transfers of one or two pages or a list.
- All host-tested; no device I/O.
