#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Clone (or update) every tpt-k repo and the TPT repos it path/git-links to,
# side by side, so that relative path dependencies resolve.
#
# Usage: scripts/clone-repos.sh [target-dir]
#   target-dir defaults to the parent of this repo (the "2 WIP" folder).
#
# Behaviour per repo:
#   - not present locally and a remote exists  -> git clone
#   - present locally                          -> git pull --ff-only (skipped if dirty)
#   - not present and no remote yet            -> reported, not created
# A repo that has a local copy but no remote is never touched.

set -u

ORG_URL="https://github.com/tpt-solutions"
ROOT="${1:-$(cd "$(dirname "$0")/../.." && pwd)}"

# tpt-k repos (one per row in todo.md's Repositories section)
TPTK_REPOS=(
  tpt-k-boot
  tpt-k-kernel
  tpt-k-root
  tpt-k-hal
  tpt-k-ipc
  tpt-k-drivers
  tpt-k-userland
  tpt-k-toolchain
)

# Existing TPT repos consumed as git/path dependencies
TPT_DEP_REPOS=(
  tpt-crypto
  tpt-crypto-rustls
  tpt-primitives
  tpt-simd
  tpt-fabric
  tpt-mcpbox
  tpt-concord
  tpt-filesystem
  tpt-cargo-vitals
  tpt-semadiff
)

ok=0
skipped=0
failed=0

sync_repo() {
  local name="$1"
  local dir="$ROOT/$name"

  if [ -d "$dir/.git" ]; then
    if [ -n "$(git -C "$dir" status --porcelain)" ]; then
      echo "[skip] $name: local changes present, not pulling"
      skipped=$((skipped + 1))
      return
    fi
    if git -C "$dir" pull --ff-only --quiet; then
      echo "[ok]   $name: up to date"
      ok=$((ok + 1))
    else
      echo "[fail] $name: pull failed"
      failed=$((failed + 1))
    fi
    return
  fi

  if [ -e "$dir" ]; then
    echo "[skip] $name: $dir exists but is not a git repo"
    skipped=$((skipped + 1))
    return
  fi

  if git ls-remote --exit-code "$ORG_URL/$name.git" >/dev/null 2>&1; then
    if git clone --quiet "$ORG_URL/$name.git" "$dir"; then
      echo "[new]  $name: cloned"
      ok=$((ok + 1))
    else
      echo "[fail] $name: clone failed"
      failed=$((failed + 1))
    fi
  else
    echo "[none] $name: no remote at $ORG_URL/$name.git yet (local-only repo)"
    skipped=$((skipped + 1))
  fi
}

echo "Target: $ROOT"
for r in "${TPTK_REPOS[@]}" "${TPT_DEP_REPOS[@]}"; do
  sync_repo "$r"
done

echo
echo "done: $ok ok, $skipped skipped, $failed failed"
[ "$failed" -eq 0 ]
