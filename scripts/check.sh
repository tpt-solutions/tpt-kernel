#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Thin wrapper: the work is done by `cargo xtask check`, in
# crates/tpt-k-toolchain-xtask, so it runs the same on Windows and Linux.
# Runs the repo's local gates: fmt, clippy, host tests, the assurance gate and the license gate.
set -euo pipefail

cd "$(dirname "$0")/.."
exec cargo run -q -p tpt-k-toolchain-xtask --bin xtask -- check "$@"
