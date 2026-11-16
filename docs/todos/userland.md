# tpt-k-userland — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: what the user sees. The shell, the POSIX shim, the network stack daemon, and
the filesystem bridge that exposes `tpt-filesystem` to the rest of the system.

Rules: `#![no_std]` is not required here, but no GPL/LGPL/AGPL/MPL or Apache-only
dependencies (dev-dependencies excepted).

---

## Phase 4 — Storage & filesystem

- [ ] `tpt-k-userland-fs-bridge`: translate IPC block and byte requests to the `tpt-filesystem` API
- [ ] IPC/FUSE-style bridge for exposing the filesystem to the rest of the system
- [ ] Mount the filesystem and read a file from user space
- [ ] Persistence test: write, reboot, read back
- [ ] Milestone M4 (userland side)

## Phase 7 — Userland, network & toolchain

- [ ] `tpt-k-userland-shell`: command parser, job control
- [ ] `tpt-k-userland`: meta-crate and interactive shell binary
- [ ] `tpt-k-userland-libc`: minimal POSIX shim over `tpt-k-ipc`
- [ ] `tpt-k-userland-net`: user-space TCP/IP stack daemon
- [ ] Zero-copy packet path: NIC driver → network stack → applications
- [ ] Network integration tests in QEMU (ping, TCP echo)
- [ ] Milestone M6 (userland side)
