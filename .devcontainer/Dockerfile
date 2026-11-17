# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Development container for tpt-kernel: the pinned Rust toolchain, both targets,
# QEMU and OVMF for the boot smoke test. The toolchain version must match
# rust-toolchain.toml.
FROM rust:1.95.0-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        qemu-system-x86 \
        ovmf \
        git \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt \
    && rustup target add x86_64-unknown-none x86_64-unknown-uefi
