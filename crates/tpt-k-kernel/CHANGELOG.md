# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Early boot entry: disables interrupts, switches to a kernel stack, and calls `kmain`.
- Panic handler and linker script for the x86_64 kernel image.
- Syscalls 1 to 18: write, exit, send, receive, yield, grant, revoke, map, spawn, spawn from a program image, create memory, send and receive with a capability, bulk send, IRQ wait, memory free, unmap, and wait.
- `SYS_MAP` refuses a second mapping of the same object at the same address, and undoes a partly mapped object on failure. Objects record up to 4 mappings.
- `SYS_UNMAP` (17): removes a mapping made by `SYS_MAP`. `SYS_MEM_FREE` (16) returns `EBUSY` while an object is still mapped.
- `SYS_MEM_FREE` (16): frees an unmapped memory object after revoking every handle to it, and returns its frames. Object 0 and mapped objects are refused.
- `SYS_WAIT` (18): returns a child's exit code once it has exited. A child the kernel killed after a fault makes the wait fail with `EFAULT`. Only the creator may wait, and only one waiter at a time.
- A child's address space is released when its creator collects the exit: its tables, and the frames only it owned. Frames shared with other spaces stay allocated.
- Failed system calls return a negated errno (`EFAULT`, `EINVAL`, `EBADF`, `EACCES`, `ENOMEM`, `ENOSPC`, `EDEADLK`, `EBUSY`, `ENOSYS`) instead of `u64::MAX`. A boot self-test checks it.
- Ring-3 exceptions go to a user fault handler, which ends the faulting thread. A boot-time user thread reads address 0 to check this.
- Exiting threads and faulting ring-3 threads delete their handles without revoking (`delete_across`), and memory objects that nothing refers to are reclaimed.
- The live capability graph is printed at boot, between `caps: begin` and `caps: end`, one `cap:` line per capability.
- Boot self-tests check handle isolation between tables, grant attenuation, errno values, memory free and reclaim, unmap, revoking a mapped object, a revoked root handle, and a faulted thread's wait.
- Boot demo: preemptive workers, a kernel server, ring-3 threads over capability handles, two isolated user address spaces, a root program that spawns children and sends a message and a bulk payload, a driver that waits for the RTC interrupt, and a fault thread.
- The `replay` Cargo feature fixes the APIC timer count, so the boot transcript repeats under an instruction counter. `replay/golden.txt` at the repository root holds a recorded transcript.
- Interrupts stay off while the bootstrap thread sets up the demo, and are enabled only when the demo threads start.

### Fixed

- A refused spawn no longer halts the kernel: the grant is checked before the child thread is created.
- Spawned children fork from a template space taken before any object is mapped, so they inherit no unrecorded mappings.
