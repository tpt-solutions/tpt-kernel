# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `parse`, `compile` and `lint` for the TOML policy manifest: objects with rights ceilings, processes with grants, the `read`, `write` and `grant` rights names.
- Errors for unknown rights, undeclared objects, grants above a ceiling, and duplicate processes.
- Lints: a grant that includes `grant`, and a declared object that no process is granted.
- A property test over every pair of rights sets (`property_grants_never_exceed_the_ceiling`), linked to the assurance graph as `MAN-1`.
