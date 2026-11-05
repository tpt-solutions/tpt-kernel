# Contributing to tpt-k

Thank you for your interest in tpt-k.

## Licensing of contributions

tpt-k is dual-licensed under **MIT OR Apache-2.0**. By submitting a contribution
(code, documentation, tests, or other material), you agree that it is licensed
under the same terms, and you confirm that you have the right to license it that way.

Every new source file must include the header:

```
// SPDX-License-Identifier: MIT OR Apache-2.0
```

## Dependencies

Before adding a dependency, check that it meets the rules in
[LICENSE-POLICY.md](LICENSE-POLICY.md). Dependencies under GPL, LGPL, AGPL, MPL,
or Apache-2.0-only licenses are rejected. CI runs `tpt-cargo-vitals` and fails on
any violation.

Kernel and trusted-computing-base crates must be `#![no_std]` and must not
include C or C++ bindings.

## Workflow

1. Open an issue or discuss the change first if it is large.
2. Keep each pull request focused on one change.
3. Make sure these pass locally before you push: `cargo fmt --check`,
   `cargo clippy`, `cargo build`, and `cargo test`.
4. Describe the change and any licensing impact in the pull request.
