# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rights, capability and handle types, and unforgeable tokens: an HMAC-SHA256 tag under a server key (SHA-256 from `tpt-primitives`), with verified attenuation. A request that is not a subset of the token's rights is reported as `NotNarrower`.
- Tokens whose rights word sets undefined bits are refused with `BadRights`, so each wire form maps to exactly one token.
- Deterministic property-style tests (seeded xorshift64, no external crates): attenuation never widens, tokens are bound to their key, any changed wire byte is detected, and attenuated tokens verify.
- Bounded exhaustive test of attenuation over every pair of defined rights masks (test-only).
