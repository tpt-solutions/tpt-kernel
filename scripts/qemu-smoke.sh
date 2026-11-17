#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Thin wrapper: the work is done by `cargo xtask qemu-smoke`, in
# crates/tpt-k-toolchain-xtask, so it runs the same on Windows and Linux.
# Builds the kernel and the loader, boots them under QEMU and OVMF, and checks the serial output. Environment overrides: QEMU, OVMF_CODE, OVMF_VARS, OUT, TIMEOUT.
set -euo pipefail

cd "$(dirname "$0")/.."
exec cargo run -q -p tpt-k-toolchain-xtask --bin xtask -- qemu-smoke "$@"
