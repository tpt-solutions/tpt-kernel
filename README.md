# tpt-k

tpt-k is a capability-based microkernel written in Rust. The kernel does only
what must run in privileged mode: memory, scheduling, IPC, capability tables
and interrupt routing. Drivers, the root server and services run in user space
and get their authority from capabilities.

It is a project by TPT Solutions, dual-licensed under
[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE). The design is in
[spec.txt](spec.txt).

## Quickstart

You need a stable Rust toolchain. The repo pins 1.95.0 in
[rust-toolchain.toml](rust-toolchain.toml), so `rustup` installs it on first use.

```sh
# 1. Targets for the bare-metal kernel and the UEFI loader
rustup target add x86_64-unknown-none x86_64-unknown-uefi

# 2. Sibling TPT repositories that the crates use by path
scripts/fetch-deps.sh

# 3. Check the machine; prints the fix for anything missing (run from the repo root)
cargo run -q -p tpt-k-toolchain --bin tpt-k-build -- doctor

# 4. Local gate: fmt, clippy (warnings denied), host tests, assurance gate
scripts/check.sh

# 5. Boot the kernel in QEMU and check the serial output (needs QEMU and OVMF)
scripts/qemu-smoke.sh
```

`scripts/check.sh` is the quick local subset of CI. The QEMU smoke test is the
end-to-end check for kernel and loader changes. The developer guide in
[docs/developer-guide.md](docs/developer-guide.md) covers prerequisites,
writing a crate and the review workflow.

## Layers

The repo is one Cargo workspace. Each crate is named `tpt-k-<layer>[-<part>]`
and belongs to one layer.

| Layer | Role | Runs in |
|---|---|---|
| `boot` | UEFI loader; builds the handoff record | UEFI firmware |
| `kernel` | Microkernel core: frames, page tables, scheduler, IPC, capabilities, x86_64 arch | ring 0 |
| `root` | First user process; creates, grants and revokes capabilities | user space |
| `hal` | Parsers for ACPI, FDT, PCI and USB descriptors | user space |
| `ipc` | Client and server framework, capability tokens, serialisation | user space |
| `drivers` | Restart policy, NVMe, VirtIO and USB HID encodings | user space |
| `userland` | Shell, libc core, network codecs, filesystem bridge | user space |
| `toolchain` | `tpt-k-build`: plans cargo commands and checks the assurance graph | host |

Per-layer notes are in [docs/layers/](docs/layers/), and the checklist for each
layer is in [docs/todos/](docs/todos/).

## Status

| Area | State |
|---|---|
| UEFI loader and kernel boot in QEMU | Working; checked by the QEMU smoke test |
| Memory, page tables, address spaces | Working in the kernel; boot memory map used |
| Scheduling | Timer-driven round-robin; priority queue host-tested, not yet wired in |
| IPC and capabilities | Working in the kernel: endpoints, grant, revoke, memory objects (map, unmap, free) |
| Drivers and root server | Not running yet; the parts are host-tested libraries |
| Shell | Host-tested REPL; external programs are not started yet |
| Parsers (ACPI, PCI, FDT, USB, FAT32, ELF, GPT, network) | Host-tested, with mutation tests |
| AArch64 | Not started |
| Real hardware | Not tested |

[todo.md](todo.md) is the authoritative list of what is open. Before a release,
[docs/release.md](docs/release.md) describes the steps and the assurance report
at [docs/assurance/report.md](docs/assurance/report.md) lists what is proven and
what is only tested.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Contributions are licensed under the
same dual license.
