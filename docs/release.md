# Versioning and release

tpt-k is one repo with one workspace. A release is a version tag on `master`
with a matching changelog entry. No crate is published to crates.io (all 32 are
`publish = false`), so a release is a source tag and the artifacts built from it.

## Versioning

- Follow [Semantic Versioning](https://semver.org/). While the major version is
  0, any minor release may change an interface, and each one says so in the
  changelog.
- The repo version is the version of the release. The crates are at `0.0.0`
  today, and that is not a release version. Before the first tag, decide whether
  to set a version in `[workspace.package]` for all crates to inherit, or to set
  it per crate. That is an open decision (see below).
- Each crate keeps its own `CHANGELOG.md`, with an `Unreleased` section. The repo
  root `CHANGELOG.md` summarises the release across crates.

## Before tagging

A release is cut only when all of these hold:

1. `scripts/check.sh` passes on `master`: formatting, clippy with warnings denied,
   host tests, and the assurance gate.
2. CI is green in every build group, including the QEMU smoke test, which boots
   the kernel under OVMF and checks the serial output.
3. The assurance report (`docs/assurance/report.md`) has been reviewed. Each
   waiver still gives a real reason, and no claim has moved from bounded-proven to
   tested without an explanation.
4. The license and CVE gate passes for this repo, with `tpt-cargo-vitals` run as
   `scripts/check.sh` runs it. The external TPT repos are a separate question. See
   `DEP-AUDIT.md`. A release that depends on a repo that fails the gate needs a
   written exception in `LICENSE-POLICY.md`.
5. `todo.md` and the layer checklists agree with the code, so every ticked item
   has a note with its date and evidence.

## Cutting a release

1. Move the `Unreleased` entries in the root `CHANGELOG.md` and in each changed
   crate's `CHANGELOG.md` under the new version heading, with the date.
2. Set the version as decided above.
3. Commit the release as one commit, with the message `release: vX.Y.Z`.
4. Create an annotated tag: `git tag -a vX.Y.Z -m "tpt-k vX.Y.Z"`.
5. Push the commit and the tag, then create the release from the tag. Attach the
   assurance report from that commit, so the proven-versus-unproven state travels
   with the release.

Steps 3 to 5 publish the code, so they need an explicit go-ahead each time.

## Open decisions

- The version number of the first release, and whether versions live in
  `[workspace.package]` or in each crate.
- Whether the root `CHANGELOG.md` or the per-crate changelogs are the release
  record of truth. This document assumes the root file summarises them.
- How to treat the external repos that fail the license gate before a tag. The
  options are to fix them, or to record a time-limited exception.
