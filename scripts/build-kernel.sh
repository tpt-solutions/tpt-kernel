#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Builds the kernel image for x86_64-unknown-none with its linker script.
# The script is passed to the kernel link only, so other crates built for the
# same target are not affected.
set -euo pipefail

cd "$(dirname "$0")/.."
exec cargo rustc -p tpt-k-kernel --bin tpt-k-kernel --target x86_64-unknown-none -- \
    -C relocation-model=static \
    -C link-arg=-Tcrates/tpt-k-kernel/linker.ld
