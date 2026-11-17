# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- FAT32 boot-partition writer (`build`): boot sector, FSInfo, backup boot sector, FAT copies, directories and 8.3 file names. Volumes are padded to the FAT32 minimum. Every image is round-trip tested through `tpt-k-boot-fs`.
