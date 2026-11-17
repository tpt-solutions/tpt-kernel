# Dev container

`devcontainer.json` and `Dockerfile` set up a container with the pinned Rust toolchain (1.95.0), both bare-metal targets, QEMU and OVMF. Open the repo in a dev-containers client (VS Code, or `devcontainer up`). The repo must sit at `/workspaces/tpt-kernel`, next to its sibling repos, so that `fetch-deps` can clone them into `/workspaces`.

On first create, `postCreateCommand` clones or updates the sibling TPT repos and installs `tpt-cargo-vitals`, which the license gate uses. If it is not available, the gate is skipped with a message, as it is on any machine without it.

Then run `cargo xtask check` and `cargo xtask qemu-smoke`. The `OVMF_CODE` and `OVMF_VARS` variables point at the Debian OVMF images.
