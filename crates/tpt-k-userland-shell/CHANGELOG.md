# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Command-line parser: words with single and double quotes and backslash escapes; pipelines; `&&`, `||`, `;`, `&`; `<`, `>`, `>>` redirections; structured errors.
- Job table: ids, running/stopped/done states, `fg`/`bg`, `%n` and `%%`/`%-` specs, and reaping of finished jobs.
