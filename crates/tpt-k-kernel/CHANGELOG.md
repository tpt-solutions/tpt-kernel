# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Failed system calls return a negated errno (`EFAULT`, `EINVAL`, `EBADF`, `EACCES`, `ENOMEM`, `ENOSPC`, `EDEADLK`, `EBUSY`, `ENOSYS`) instead of `u64::MAX`. A boot self-test checks it.
- `SYS_MEM_FREE` (number 16): frees an unmapped memory object after revoking every handle to it, and returns its frames. Object 0 and mapped objects are refused.
- Ring-3 exceptions go to a user fault handler, which ends the faulting thread. A boot-time user thread reads address 0 to check this.

- Early boot entry: disables interrupts, switches to a kernel stack, and calls `kmain`.
- Phase 2 demo: preemptive workers, a kernel server, ring-3 threads over capability handles, and two isolated user address spaces.
- Panic handler and linker script for the x86_64 kernel image.
