#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Thin wrapper: the work is done by `cargo xtask build-kernel`, in
# crates/tpt-k-toolchain-xtask, so it runs the same on Windows and Linux.
# Builds the kernel for x86_64-unknown-none with its linker script.
set -euo pipefail

cd "$(dirname "$0")/.."
exec cargo run -q -p tpt-k-toolchain-xtask --bin xtask -- build-kernel "$@"
