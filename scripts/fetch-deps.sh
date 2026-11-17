#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Thin wrapper: the work is done by `cargo xtask fetch-deps`, in
# crates/tpt-k-toolchain-xtask, so it runs the same on Windows and Linux.
# Clones or fast-forwards the sibling TPT repos. Optional argument: the target folder (default: the parent of this repo).
set -euo pipefail

cd "$(dirname "$0")/.."
exec cargo run -q -p tpt-k-toolchain-xtask --bin xtask -- fetch-deps "$@"
