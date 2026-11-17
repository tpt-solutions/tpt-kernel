# tpt-k developer guide

This guide covers what you need to build, test and extend tpt-k as it stands.
It describes what exists. Open work is tracked in [todo.md](../todo.md) and the
per-layer checklists in [docs/todos/](todos/).

## What is and is not built

- **Built and host-tested:** most parsers and codecs (firmware tables, PCI,
  FDT, USB descriptors, FAT32, GPT, Ethernet/IPv4/UDP/TCP/ARP), the capability
  and IPC framework, the root capability service, the restart policy, the
  process lifecycle state machine, the shell parser, and the build planner.
- **Built for x86_64 and checked in CI, but not run here:** the UEFI loader and
  the kernel. Booting them needs QEMU and OVMF (see below).
- **Not built yet:** the WASM driver host, device I/O for NVMe, VirtIO and USB,
  the kernel's use of the scheduler queue and the process lifecycle, process
  spawning in the shell, AArch64, and running on real hardware.

Most crates are libraries with unit tests. A crate's README `Status` section
says what it provides and what it does not.

## Prerequisites

- A stable Rust toolchain with the `x86_64-unknown-none` and
  `x86_64-unknown-uefi` targets:

  ```sh
  rustup target add x86_64-unknown-none x86_64-unknown-uefi
  ```

- The sibling TPT repositories, which the crates use by path. Clone or update
  them next to this repo with `scripts/fetch-deps.sh`.
- For the boot smoke test only: `qemu-system-x86_64` and an OVMF firmware image.
- For the license gate only: `tpt-cargo-vitals` on `PATH`.

## Layout

```
crates/            one crate per unit, named tpt-k-<layer>[-<part>]
docs/layers/       what each layer owns
docs/todos/        the checklist for each layer
scripts/           build, check, QEMU smoke test and dependency fetch
.githooks/         optional pre-push hook
```

Layers are `boot`, `kernel`, `root`, `hal`, `ipc`, `drivers`, `userland` and
`toolchain`. A crate's name starts with its layer, and the layer decides which
rules apply (see below).

## Checking your change

Run the local gate before you push:

```sh
scripts/check.sh
```

It runs `cargo fmt --check`, clippy with warnings denied, and the host test
suite over the workspace, excluding the two bare-metal images (`tpt-k-boot` and
`tpt-k-kernel`), which build only for their own targets. When `tpt-cargo-vitals`
is on `PATH` it also runs the license and CVE gate with the allowlist from
[LICENSE-POLICY.md](../LICENSE-POLICY.md).

To use it as a pre-push hook:

```sh
git config core.hooksPath .githooks
```

CI also builds the kernel-side crates for `x86_64-unknown-none` and checks
that they are `#![no_std]`. Run that locally with:

```sh
cargo clippy -p <crate> --target x86_64-unknown-none -- -D warnings
```

The same script runs the assurance gate (see below). Its report is written to
`docs/assurance/report.md`, which is generated, so do not edit it by hand.

## Assurance gate

[docs/assurance/graph.json](assurance/graph.json) links each claim about the
capability and IPC code to the tests, exhaustive checks and fuzz targets that
back it. The invariants themselves are in
[docs/assurance/invariants.md](assurance/invariants.md).

```sh
cargo run -q -p tpt-k-toolchain --bin tpt-k-build -- assure --report docs/assurance/report.md
```

The gate checks that every named function exists in its file, and it sorts
each claim as bounded-proven, tested, waived or unproven. It fails if a critical
claim is not bounded-proven and has no waiver. A waiver must say why the claim
stays open. `cargo test` runs the same check on the real graph, so CI enforces
it too.

To add a claim, add an entry to `graph.json` that names real evidence. Use
`exhaustive` only for a check that enumerates a finite model, and `test` for
sampled checks. A sampled check does not make a claim proven.

## Reviewing generated or refactored code

For a large generated or refactored change, review the semantics rather than
the text. The repo is set up to use `tpt-semadiff` as the git difftool:

```sh
git difftool --tool=semadiff <base>..<head>
```

The `difftool.semadiff.cmd` setting is already in the git config of this
clone. The `tpt-semadiff` binary has to be on `PATH`. It was not on PATH when
this section was written, so the workflow has not been run on this machine.

## Building the kernel and loader

```sh
scripts/build-kernel.sh          # kernel for x86_64-unknown-none with its linker script
```

The smoke test builds the kernel and the UEFI loader, stages an EFI System
Partition, boots it under OVMF and checks for the `Hello tpt-k` line and the
success exit code:

```sh
QEMU=qemu-system-x86_64 OVMF_CODE=... OVMF_VARS=... scripts/qemu-smoke.sh
```

See the header of `scripts/qemu-smoke.sh` for all overrides. Nothing in this
repo runs QEMU without you asking for it.

## Writing a crate

- Start every source file with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- Kernel and trusted-computing-base crates are `#![no_std]` with no C or C++
  bindings. Use `#[cfg(test)] extern crate std;` so host tests still run.
- Document every public item. Every `unsafe` block needs a comment that states
  the invariant it relies on (the kernel has none so far).
- Keep parsers allocation-free and bounds-checked. Return an error rather than
  panicking on hostile input, and add a test for each refusal.
- Hardware access goes through a trait the caller implements (for example
  `ConfigAccess` in the PCI crate). That keeps the parsing testable on the host.
- Each crate has a `README.md` with a `Status` section and a `CHANGELOG.md`
  with an `Unreleased` section. Keep both accurate: say what is not built yet.
- Add a dependency only after checking [LICENSE-POLICY.md](../LICENSE-POLICY.md).
  Apache-2.0-only dependencies are allowed only as dev-dependencies.

## Working with capabilities and IPC

- A capability is an object and a set of rights (`tpt_k_ipc_cap::Rights`).
  Rights can only be narrowed, never widened.
- A token is a capability with an HMAC-SHA256 tag under a server key. A service
  verifies the token before it checks the rights, and rejects a token made
  under another key.
- A service implements `tpt_k_ipc::Service`: it names the rights each opcode
  needs and handles the request. `tpt_k_ipc::serve` does the checks and
  builds the reply, so a refusal is still a reply.
- The root capability service (`tpt-k-root`) shows the pattern. MINT needs its
  own right, so GRANT alone cannot mint arbitrary rights.

## Adding a driver

The pieces are in place, but the driver host is not built, so a driver cannot
run yet. The layers you would use:

1. Describe the hardware: `tpt-k-hal-*` parses the firmware tables and the bus
   configuration, and `tpt-k-hal` provides the resource types and the conflict
   rule.
2. Encode the device's commands and queues in `tpt-k-drivers-<device>`. The
   NVMe, VirtIO and USB HID crates show the pattern: encodings and state, with
   no device I/O yet.
3. Serve the driver's block or byte operations through `tpt-k-ipc`
   (`block` module for block devices).
4. Supervise the driver with the restart policy in `tpt-k-drivers`. It is a
   state machine that takes ticks and returns actions.

The WASM driver host, interrupt delivery to user space, and device I/O are open
items in [docs/todos/drivers.md](todos/drivers.md).

## Licensing of contributions

Contributions are licensed under MIT OR Apache-2.0. See
[CONTRIBUTING.md](../CONTRIBUTING.md).
