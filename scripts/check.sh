#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Local gate: formatting, clippy with warnings denied, the host test suite, and
# the license and CVE gate when tpt-cargo-vitals is on PATH. CI runs the same
# checks plus the target-specific no_std builds, so this script is the quick
# local subset, not a replacement for CI.
#
# Usage: scripts/check.sh
set -euo pipefail

cd "$(dirname "$0")/.."

# Bare-metal images are built for their own targets in CI, not for the host.
BARE_METAL="--exclude tpt-k-boot --exclude tpt-k-kernel"

LICENSES="MIT,Unicode-3.0,Unicode-DFS-2016,BSD-2-Clause,BSD-3-Clause,ISC,Zlib,Unlicense,CC0-1.0"

echo "== cargo fmt --check"
cargo fmt --all -- --check

echo "== cargo clippy (host, warnings denied)"
cargo clippy --workspace $BARE_METAL --all-targets -- -D warnings

echo "== cargo test (host)"
cargo test --workspace $BARE_METAL

if command -v tpt-cargo-vitals >/dev/null 2>&1; then
    echo "== tpt-cargo-vitals license and CVE gate"
    tpt-cargo-vitals check --path . --enforce-license "$LICENSES" --ci-mode
else
    echo "== tpt-cargo-vitals not on PATH: license gate skipped"
fi

echo "check: all local gates passed"
