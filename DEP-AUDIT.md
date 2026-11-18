# TPT Dependency Audit

Audited against [LICENSE-POLICY.md](LICENSE-POLICY.md). Date: 2026-11-06. Repos cloned from `github.com/tpt-solutions/` into `c:\Programming\2 WIP\`.

Gate used: `tpt-cargo-vitals check --enforce-license "MIT,Unicode-3.0,Unicode-DFS-2016,BSD-2-Clause,BSD-3-Clause,ISC,Zlib,Unlicense,CC0-1.0" --ci-mode`.
`Apache-2.0 WITH LLVM-exception` is approved by the maintainer, but see "Tool limitation" below.

## Summary

| Repo | Gate | CVEs | `no_std` libs passing |
|---|---|---|---|
| tpt-primitives | FAIL (4 Apache-only) | 0 | 0 / 2 |
| tpt-simd | FAIL (3 Apache-only, ryu) | 0 | 41 / 43 |
| tpt-fabric | pass | 0 | 1 / 13 (core only; the rest are std) |
| tpt-mcpbox | FAIL (many) | **5** | 0 / 7 (std by design) |
| tpt-concord | FAIL (MIT-0, ryu, sync_wrapper) | 0 | 0 / 15 (std by design) |
| tpt-filesystem | pass | 0 | 0 / 3 (std daemon) |

## Blocking findings

1. **tpt-mcpbox: 5 known CVEs** in `wasmtime 48.0.7` and `wasmtime-wasi 48.0.7`/`wasmtime-wasi-http`. Fixed in `>= 48.0.8`:
   RUSTSEC-2026-0420, -0421, -0422, -0423, -0424. Fix: bump `wasmtime` to 48.0.8 or later and refresh `Cargo.lock`.
2. **RESOLVED 2026-11-16: tpt-primitives is `no_std`.** It now uses `cbor4ii` and an in-crate CBOR tree; `ciborium` is removed. Canonical bytes are unchanged. Original finding: tpt-primitives cannot be `no_std`. It depends on `ciborium` 0.2.2, and `ciborium-io` requires `std`. The spec puts this crate under `tpt-k-ipc-cap` and `tpt-k-root-caps`, so this blocks the Phase 2 capability work. Options: switch to a `no_std` CBOR crate, or make `ciborium` optional behind a `std` feature and provide a `no_std` encoder.
3. **Apache-2.0-only dependencies** (policy denies these) in primitives, simd, mcpbox, concord:
   `ciborium`, `ciborium-io`, `ciborium-ll`, `borsh-derive`, `sync_wrapper`, `rpassword`, `rtoolbox`, `unicode-general-category`, `witx`, `ring` (`Apache-2.0 AND ISC`), `regorus` (`MIT AND Apache-2.0 AND BSD-3-Clause`).
   Each needs a replacement, or a documented exception in LICENSE-POLICY.md.

## Policy decisions

Decided 2026-11-18, following the recommendations:

- **`MIT-0`**: allowed. It is permissive, and `LICENSE-POLICY.md` allows it. The
  tpt-k gate (`xtask check` and the CI license job) now includes it.
- **`BSL-1.0`**: allowed, for `ryu` (`Apache-2.0 OR BSL-1.0`). It is in
  `LICENSE-POLICY.md` and in the tpt-k gate.
- **`CDLA-Permissive-2.0`**: not added to the gate. It is a data license, and
  the approved exception is for `webpki-roots` in `tpt-mcpbox` only. A global
  allowlist entry would also allow it everywhere, so the exception stays scoped
  in `LICENSE-POLICY.md`.

## Status 2026-11-18, after the gate fixes

Apache-2.0 alone is allowed only in dev-dependencies (LICENSE-POLICY.md). The
gate now honours that, and two gate bugs that gave false violations are fixed in
`tpt-cargo-vitals` (commits `088deb6` and `b718a87` on its master):

- `--dev-allow Apache-2.0` judges a dependency no build ships against the
  policy plus Apache-2.0. A shipped dependency never gets that allowance.
- A dependency that no enabled feature turns on is listed by `cargo metadata`,
  but never compiled. It is now skipped, not checked.
- `Apache-2.0 WITH LLVM-exception` is judged as one license, so the policy can
  allow it exactly.
- `--exception CRATE=LICENSE` lets one crate use one license, for the recorded
  `webpki-roots` exception.

Results, with the policy allowlist, `--dev-allow Apache-2.0`, and the
`webpki-roots` exception for `tpt-mcpbox` only:

| Repo | License gate | Advisories | Note |
|---|---|---|---|
| tpt-primitives | pass | 0 | |
| tpt-simd | pass | 0 | |
| tpt-concord | pass | 0 | |
| tpt-crypto | pass | 0 | |
| tpt-fabric | pass | 0 | |
| tpt-filesystem | pass | 0 | |
| tpt-crypto-rustls | pass | 7 (see below) | `deny.toml` added in `936589f`; `cargo deny check` passes |
| tpt-mcpbox | pass (`witx` and `webpki-roots` as recorded exceptions) | 1 shipped, accepted (`rsa`) | moved to `tpt-crypto-rustls` on 2026-11-18; `cargo deny` passes |

The sibling repos run `cargo-deny` in CI, not vitals. `cargo deny check` passes
in primitives, simd, crypto, fabric, filesystem, mcpbox and crypto-rustls. vitals
does not read `deny.toml`, so its CVE count still lists the accepted advisories.

`witx` is a normal dependency: `wasmtime-wasi` → `wiggle` → `wiggle-generate`
→ `wiggle-macro` (a proc macro, so it runs at build time and is not linked into
the binary). It is an exception until `tpt-wasm` replaces Wasmtime (LICENSE-POLICY.md).

## Advisories: what ships and what is dev-only

Checked with `cargo tree --target all -e all` on 2026-11-18.

- **Done, `tpt-mcpbox` (2026-11-18):** the outbound TLS client moved from
  `rustls-rustcrypto` 0.0.2-alpha to `tpt-crypto-rustls` (`02f0db0` on its
  master). That removed `rustls-webpki` 0.102.8 (five advisories), `crypto-bigint`
  and `paste` from the shipped tree, and their entries are gone from `deny.toml`.
  The workspace builds, and all of its tests pass. A scratch program completed
  TLS 1.3 handshakes with example.com, www.google.com and api.github.com, each with
  certificate validation against the bundled roots. Only `rsa` remains, through
  `tpt-crypto-rustls`, and `deny.toml` accepts it until 2027-02-09.
- **Caveat:** the migration puts an unaudited, client-only provider on the
  outbound path. `tpt-crypto-rustls`'s README says not to use it to protect real
  traffic until it is audited. `tpt-mcpbox` depends on it by path, so CI needs a
  git dependency before it can build the repo.
- **Shipped, `tpt-crypto-rustls`:** `rsa` 0.9.10 (RUSTSEC-2023-0071, Marvin). The
  crate uses it only to verify signatures. The fix is rsa 0.10, which needs
  digest 0.11 support in `tpt-crypto`. rsa 0.10.0 does not build against the
  current digests, so that migration comes first.
- **Dev-only, `tpt-crypto-rustls`:** the other six advisories come from
  `rustls-rustcrypto` as a dev-dependency, the peer in the handshake tests. They
  are accepted in its `deny.toml` until 2027-02-09 and do not ship.

**Recommendation.**
1. Done: `tpt-mcpbox` uses `tpt-crypto-rustls` (see above). Still to do: the
   audit that the README asks for before the provider protects real traffic, and
   a git dependency in place of the path dependency.
2. Move `tpt-crypto` to digest 0.11 so `rsa` can go to 0.10. That clears the last
   shipped advisory in `tpt-crypto-rustls`.
3. Keep the dev-only acceptances in `tpt-crypto-rustls` until the test peer is
   replaced. They do not affect shipped code.
4. Have vitals read `deny.toml` ignores, with their reasons, so the CVE count
   agrees with cargo-deny. Not done; it is a vitals change.

The CI gate in each repo still runs the old `tpt-cargo-vitals` flags, because
CI installs vitals from GitHub. The new flags go into CI only after the vitals
commits are pushed. Pushing is a separate step that has not been done.

## Tool limitation

`tpt-cargo-vitals` ignores `WITH` exceptions and judges only the base license. So `Apache-2.0 WITH LLVM-exception` is evaluated as plain `Apache-2.0`. Listing it in `--enforce-license` requires listing `Apache-2.0`, which would also let Apache-only dependencies through. The gate was therefore left strict, and the cranelift/wasmtime crates still show as violations. Fix in vitals: match the full `X WITH Y` expression against the allowlist.

## Update 2026-11-16

- **tpt-mcpbox:** `wasmtime` is at 48.0.8, so the five RUSTSEC wasmtime advisories are gone. The remaining advisories come from `rustls-rustcrypto` 0.0.2-alpha (`rsa`, `crypto-bigint`, `rustls-webpki 0.102.8`), accepted in `tpt-mcpbox/deny.toml` until 2027-02-09. `cargo deny check advisories licenses` passes. `tpt-cargo-vitals` does not read `deny.toml`, so its gate still reports these.
- **tpt-primitives:** no longer depends on `ciborium`; builds `no_std`.

## `no_std` results

Per library crate, `cargo check --lib --target x86_64-unknown-none --no-default-features`.

- **tpt-simd**: passes 41 of 43. Failing: `tpt-simd-testutil`, and one other (not investigated).
- **tpt-fabric**: passes `tpt-fabric-core` only. The other 12 fail with `can't find crate for std`.
- **tpt-primitives**: fails on `ciborium` (see blocking finding 2).
- **tpt-concord, tpt-mcpbox, tpt-filesystem**: all fail with `can't find crate for std`. These are services and tooling, so std is expected. Not a problem unless the spec requires them in the TCB.

## Not yet done

- The `tpt-cargo-vitals` run time is about 5–7 s, not the `<100 ms` target in the todo. Measured on a repo with zero dependencies, so the cost is fixed overhead in vitals, not dependency work (`cargo metadata` alone takes 0.13 s).
- WASM runtime audit: wasmtime is the runtime in mcpbox. Its license is covered above, and its CVEs are blocking finding 1.
