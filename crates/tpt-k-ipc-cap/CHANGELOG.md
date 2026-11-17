# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rights, capability and handle types, and unforgeable tokens: an HMAC-SHA256 tag under a server key (SHA-256 from `tpt-primitives`), with verified attenuation. The `CapabilityId` and CBOR types of `tpt-primitives` are not used yet.
