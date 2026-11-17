# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Driver restart policy: a fixed-capacity `Manager` of `Driver`s with heartbeats, exponential backoff, a stable-run reset, give-up after too many crashes, and stop requests.
