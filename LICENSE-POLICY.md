# tpt-k License Policy

Owner: TPT Solutions · Applies to: all `tpt-k-*` repositories and crates

## Project license

Every project source file, crate, and repository is dual-licensed under
**`MIT OR Apache-2.0`**. Each `Cargo.toml` sets:

```toml
license = "MIT OR Apache-2.0"
authors = ["TPT Solutions"]
```

Each repository ships `LICENSE-MIT` and `LICENSE-APACHE` at its root.

## Source file header

Every source file begins with:

```
// SPDX-License-Identifier: MIT OR Apache-2.0
```

Use `#` instead of `//` in shell scripts, TOML, YAML, and Python files.

## Dependency allowlist

These licenses may be used in dependencies, including transitive ones:

- MIT
- Apache-2.0, only when dual-licensed with MIT (`MIT OR Apache-2.0`)
- BSD (2-clause and 3-clause)
- ISC
- Zlib
- Unlicense
- CC0-1.0
- Unicode (Unicode-DFS-2016 and similar)

## Dependency denylist

These are prohibited in any dependency, including transitive ones, and in
any code that is linked into the kernel or trusted computing base:

- GPL (any version)
- LGPL (any version)
- AGPL (any version)
- MPL (any version)
- Apache-2.0 alone, without an MIT option
- Any license not on the allowlist

## Enforcement

- Dependency licenses are checked by `tpt-cargo-vitals check --enforce-license MIT --ci-mode`
  in CI and locally before push.
- Kernel and TCB crates are `#![no_std]`, with no C or C++ bindings.
- Existing TPT repositories (`tpt-primitives`, `tpt-simd`, `tpt-fabric`,
  `tpt-mcpbox`, `tpt-concord`, `tpt-filesystem`) are consumed as git or path
  dependencies. They are already `MIT OR Apache-2.0`, but their transitive
  dependencies must still be audited (see the todo checklist).
- A license violation fails the build. It is not a warning.

## Exceptions

No exceptions are granted by default. A proposed exception requires a written
justification in the pull request, and a maintainer must approve it. An approved
exception is recorded in this file with its date and reason.

## Open items

- Trademark and NOTICE handling for "tpt-k" and "TPT Solutions" is not yet decided.
