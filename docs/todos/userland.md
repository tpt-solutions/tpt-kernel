# tpt-k-userland — Checklist

Owner: TPT Solutions · License: `MIT OR Apache-2.0`
Master plan and milestones: [tpt-kernel/todo.md](../tpt-kernel/todo.md) · Spec: [spec.txt](../tpt-kernel/spec.txt)

Scope: what the user sees. The shell, the POSIX shim, the network stack daemon, and
the filesystem bridge that exposes `tpt-filesystem` to the rest of the system.

Rules: `#![no_std]` is not required here, but no GPL/LGPL/AGPL/MPL or Apache-only
dependencies (dev-dependencies excepted).

---

## Phase 4 — Storage & filesystem

- [ ] `tpt-k-userland-fs-bridge`: translate IPC block and byte requests to the `tpt-filesystem` API (partly, 2026-11-17: a byte-range to sector mapper, a write-through LRU sector cache and `read_bytes` over the `block` protocol, host-tested; no `tpt-filesystem` integration yet)
- [ ] IPC/FUSE-style bridge for exposing the filesystem to the rest of the system
- [ ] Mount the filesystem and read a file from user space
- [ ] Persistence test: write, reboot, read back
- [ ] Milestone M4 (userland side)

## Phase 7 — Userland, network & toolchain

- [ ] `tpt-k-userland-shell`: command parser, job control (partly, 2026-11-17: quoting, pipelines, `&&`/`||`/`;`, background and redirections; a job table with fg/bg and reaping. No process spawning yet)
- [ ] `tpt-k-userland`: meta-crate and interactive shell binary (partly, 2026-11-17: a REPL library and a stdin-driven binary with builtins; external commands are reported as not supported and there is no process spawning)
- [ ] `tpt-k-userland-libc`: minimal POSIX shim over `tpt-k-ipc` (partly, 2026-11-17: memory and string functions, strtol-style parsing, integer formatting, errno values and path normalisation, host-tested. 2026-11-18: C-named string and memory exports are behind the `c-abi` feature, with no errno export, so the "no C ABI yet" part is stale)
- [ ] `tpt-k-userland-net`: user-space TCP/IP stack daemon (partly, 2026-11-17: Ethernet, ARP, IPv4, ICMP, UDP and TCP-header codecs, the Internet checksum, and an ARP cache; a TCP connection state machine without retransmission or congestion control; no daemon)
- [ ] Zero-copy packet path: NIC driver → network stack → applications
- [ ] Network integration tests in QEMU (ping, TCP echo)
- [ ] Milestone M6 (userland side)

## Phase 11a — stub cleanup (2026-11-18)

- [x] `Spawner` trait: non-builtin commands go to it; `NoSpawner` is the default. Pipes and redirections are still refused, so building them is the next step
- [x] Binary writes the prompt only when stdin is a terminal (`IsTerminal`)
- [x] `tpt-k-userland-libc`: C-named string and memory exports behind the `c-abi` feature (no errno export)
