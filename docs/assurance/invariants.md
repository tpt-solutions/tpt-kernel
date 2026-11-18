# Capability and IPC invariants

This page states the invariants that the capability and IPC code must keep. The
machine-readable form is [graph.json](graph.json), and the checked result is the
generated [report.md](report.md). When a claim changes, edit `graph.json` and run
`scripts/check.sh` (or `tpt-k-build assure`). The gate fails if a claim's
evidence goes missing, or if a critical claim is not bounded-proven and has no
waiver.

Read a status this way:

- **bounded-proven**: an exhaustive check over a finite model covers the claim.
  It holds for the stated bound, not for every possible system.
- **tested**: sampled evidence only (unit, property-style or mutation tests). It
  is not a proof.
- **waived**: the claim stays open on purpose, and the waiver says why.

## Model bounds

| Check | Bound |
|---|---|
| Capability table (`CAP-*`) | 2 tables of 3 slots, every operation sequence up to depth 3, over 2 objects and 4 rights sets. Derivation is checked on every pair of the 8 rights masks. |
| Endpoint (`IPC-*`) | 1 endpoint with queues of 2, 3 threads, every operation sequence up to depth 6. |

These are the tests `bounded_exhaustive_capability_invariants`,
`derive_accepts_exactly_grantable_subsets` and `bounded_exhaustive_endpoint_invariants`.
The two exhaustive searches assert a minimum number of explored steps, so an
empty search cannot pass.

## Capability invariants (`crates/tpt-k-kernel-ipc/src/cap.rs`)

- **CAP-1**: Deriving a capability never yields a right its parent lacks, and it
  needs `GRANT` on the parent. *Critical.*
- **CAP-3**: Revoking a capability revokes everything derived from it, directly
  or transitively. No derived capability outlives its parent. *Critical.*
- **CAP-4**: A handle to a revoked slot is refused, even after the slot is reused.
  Forged and out-of-range handles name no live capability. *Critical.*
- **CAP-5**: Revoking a capability revokes every copy made from it, in any
  table, transitively. No copy outlives its origin. *Critical.*
- **CAP-6**: A derived capability or copy names the same object as its source.
- **CAP-7**: A copy across tables carries no right its origin lacks. A grant needs
  `GRANT` on the source. *Critical.*
- **CAP-8**: A refused capability operation leaves the table unchanged. *Critical.*

## IPC invariants (`crates/tpt-k-kernel-ipc/src/endpoint.rs`)

- **IPC-1**: An endpoint neither loses nor duplicates a message. Each message sent
  is queued, held, received or taken, exactly once. *Critical.*
- **IPC-2**: A received message comes from the sender it names. *Critical.*
- **IPC-3**: Queues are first-in, first-out and compact, so senders' messages are
  received in send order. *Critical.*
- **IPC-4**: A thread waits in at most one place on an endpoint. *Critical.*
- **IPC-5**: A message is held only for the receiver at the head of the queue, and
  a thread takes a message only when one was held for it. *Critical.*
- **IPC-6**: A send or receive refused because a queue is full leaves the endpoint
  unchanged. *Critical.*

## Token and service invariants

- **TOK-1** (`crates/tpt-k-ipc-cap`): An altered token, or one made under another
  key, is refused. *Critical, waived.* Sampled tests only. HMAC-SHA256 matches
  RFC 4231, but the scheme is not model-checked.
- **TOK-2** (`crates/tpt-k-ipc-cap`): Attenuating a token never yields a right
  outside the source's rights. *Critical, bounded-proven:* checked over every pair
  of the 64 rights masks, in `attenuation_is_exact_over_all_rights_pairs`.
- **SER-1** (`crates/tpt-k-ipc-serde`): The frame decoder and its accessors never
  panic on hostile or mutated input. *Critical, waived.* Mutation tests only. No
  coverage-guided fuzzing has been run.
- **SRV-1** (`crates/tpt-k-ipc`): A service body runs only with a valid token that
  carries the rights the opcode needs. *Critical, bounded-proven:* checked over 64
  rights masks, 4 opcodes (3 offered) and 2 keys, in
  `service_body_runs_exactly_when_token_is_valid_and_rights_allow`.

## Rights, wait and parser claims

- **RIGHTS-1** (`crates/tpt-k-kernel-ipc`, `crates/tpt-k-ipc-cap`, checked by
  `docs/assurance/proofs/rights.py`): For rights words of up to 64 bits, derive
  and grant never produce a right the source lacks, derive needs `GRANT`, and token
  attenuation never widens. *Critical, bounded-proven.* This is a z3 proof of the
  rights bit model, not of the Rust code. The gate reads the verdicts from
  `rights.py.log` and fails unless each named property is `unsat` there.
- **WAIT-1** (`crates/tpt-k-kernel`): A thread can wait only on a thread it
  created. A wait returns the child's exit code, and a child the kernel ended after
  a fault makes the wait fail with `Fault`. *Not critical, tested.* The evidence is
  a boot self-test in the kernel, so it runs under QEMU.
- **FUZZ-1** (the ACPI, FDT, PCI and USB parsers, and `tpt-k-ipc-serde`): The
  parsers never panic on mutated input. *Not critical, tested.* The evidence is
  seeded mutation campaigns of one million iterations per parser. The logs are in
  `docs/assurance/campaigns/`. The campaigns are not coverage-guided.

## Not yet done

- **Specification in `tpt-concord`.** These invariants are recorded here, not in
  the sibling `tpt-concord` repo. Its CLI is not built on this machine, and the
  integration is still open.
- **Formal model and SMT/model checking.** The capability and endpoint checks
  above are bounded exhaustive searches over an executable model, not proofs. The
  only solver proof is RIGHTS-1, which covers the rights bit model and not the
  Rust code. It is run by hand, and its verdicts are recorded in `rights.py.log`.
- **Coverage-guided fuzzing.** The seeded mutation campaigns are run, but no
  coverage-guided fuzzer is, and the syscall interface has no fuzz target.
  `cargo-fuzz` needs a nightly Rust toolchain and a sanitizer runtime, which this
  Windows machine does not have.
- **TOK-1 (forgery and tampering)** is sampled only. The HMAC construction is
  not model-checked, and the checks use two keys.
- **The kernel's own syscall dispatch** is not in the model. The model covers the
  `tpt-k-kernel-ipc` tables and endpoints that the dispatch calls.

## Related

- [Design specification](../../spec.txt)
- [Layer checklist: kernel](../todos/kernel.md), [ipc](../todos/ipc.md)
- [Master checklist](../../todo.md), Phase 5
