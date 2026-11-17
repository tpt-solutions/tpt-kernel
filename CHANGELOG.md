# Changelog

All notable changes to tpt-k are recorded here. Each crate also keeps its own
`CHANGELOG.md`. This file covers the repo as a whole: the workspace, the
assurance gate and the release process.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The repo has no release yet, so versions follow [release.md](docs/release.md).

## [Unreleased]

### Added

- Bounded exhaustive checks of the capability table (`CAP-1` to `CAP-8`) and of
  endpoints (`IPC-1` to `IPC-6`), in `tpt-k-kernel-ipc`. They run every operation
  sequence up to a stated depth, and check the invariants after each step.
- The assurance graph, `docs/assurance/graph.json`, which links each claim to
  its evidence. The invariants are described in `docs/assurance/invariants.md`.
- `tpt-k-build assure`: the assurance gate. It checks that the evidence exists,
  reports each claim as bounded-proven, tested, waived or unproven, and fails on
  an unproven critical claim with no waiver. The gate runs in `scripts/check.sh`
  and in `cargo test`. The generated report is `docs/assurance/report.md`.
- `docs/architecture.md`, `docs/release.md`, and the semadiff review workflow in
  the developer guide.

### Changed

- The layer docs in `docs/layers/` no longer call the crates empty placeholders.
  They point to the layer checklists and crate READMEs, which are kept current.
- Capability slots compare for equality, so the invariant checks can see whether
  a refused operation changed the table.

### Not yet

- Coverage-guided fuzzing campaigns, an SMT or machine-checked formal model, and
  the `tpt-concord` specification of these invariants. See
  `docs/assurance/invariants.md`.
- The external TPT repos do not all pass the license gate yet. See `DEP-AUDIT.md`.
