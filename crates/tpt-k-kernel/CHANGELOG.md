# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `SYS_UNMAP` (number 17): removes the mapping of a memory object at a given address. Objects record up to 4 mappings, and `SYS_MEM_FREE` returns `EBUSY` until all are removed. A boot self-test checks it.
- `SYS_MAP` refuses a second mapping of the same object at the same address, and undoes a partly mapped object on failure.
- The live capability graph is printed at boot, between `caps: begin` and `caps: end`, one `cap:` line per capability.
- Exiting threads and faulting ring-3 threads delete their handles without revoking (`delete_across`), and memory objects that nothing refers to are reclaimed.
- Interrupts stay off while the bootstrap thread sets up the demo, and are enabled only when the demo threads start.

- Failed system calls return a negated errno (`EFAULT`, `EINVAL`, `EBADF`, `EACCES`, `ENOMEM`, `ENOSPC`, `EDEADLK`, `EBUSY`, `ENOSYS`) instead of `u64::MAX`. A boot self-test checks it.
- `SYS_MEM_FREE` (number 16): frees an unmapped memory object after revoking every handle to it, and returns its frames. Object 0 and mapped objects are refused.
- Ring-3 exceptions go to a user fault handler, which ends the faulting thread. A boot-time user thread reads address 0 to check this.

- Early boot entry: disables interrupts, switches to a kernel stack, and calls `kmain`.
- Phase 2 demo: preemptive workers, a kernel server, ring-3 threads over capability handles, and two isolated user address spaces.
- Panic handler and linker script for the x86_64 kernel image.

### Fixed

- A refused spawn no longer halts the kernel: the grant is checked before the child thread is created.
- Spawned children fork from a template space taken before any object is mapped, so they inherit no unrecorded mappings.
