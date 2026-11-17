// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded exhaustive checks of the capability-table invariants.
//!
//! Every sequence of up to [`DEPTH`] operations over [`TABLES`] tables of
//! [`SLOTS`] slots is run, and the invariants are checked after each step. The
//! model is finite, so the result holds for this bound and no larger. The
//! invariant IDs (CAP-n) are the ones in `docs/assurance/invariants.md`.

use super::*;

const TABLES: usize = 2;
const SLOTS: usize = 3;
const DEPTH: usize = 3;
type State = [CapTable<SLOTS>; TABLES];

const OBJECTS: [ObjectId; 2] = [ObjectId(1), ObjectId(2)];
const RIGHTS: [Rights; 4] = [
    Rights::ALL,
    Rights::READ.union(Rights::GRANT),
    Rights::READ,
    Rights::WRITE,
];

/// One operation. Handles are named by slot index, and an operation on an
/// empty slot is not generated.
#[derive(Clone, Copy, Debug)]
enum Op {
    Mint {
        table: usize,
        object: usize,
        rights: usize,
    },
    Derive {
        table: usize,
        slot: usize,
        rights: usize,
    },
    Grant {
        table: usize,
        slot: usize,
        rights: usize,
    },
    Revoke {
        table: usize,
        slot: usize,
    },
}

const MINTS: usize = TABLES * OBJECTS.len() * RIGHTS.len();
const DERIVES: usize = TABLES * SLOTS * RIGHTS.len();
const GRANTS: usize = TABLES * SLOTS * RIGHTS.len();
const REVOKES: usize = TABLES * SLOTS;
const OP_COUNT: usize = MINTS + DERIVES + GRANTS + REVOKES;

fn decode(mut k: usize) -> Op {
    if k < MINTS {
        let rights = k % RIGHTS.len();
        k /= RIGHTS.len();
        let object = k % OBJECTS.len();
        let table = k / OBJECTS.len();
        return Op::Mint {
            table,
            object,
            rights,
        };
    }
    k -= MINTS;
    if k < DERIVES {
        let rights = k % RIGHTS.len();
        k /= RIGHTS.len();
        return Op::Derive {
            table: k / SLOTS,
            slot: k % SLOTS,
            rights,
        };
    }
    k -= DERIVES;
    if k < GRANTS {
        let rights = k % RIGHTS.len();
        k /= RIGHTS.len();
        return Op::Grant {
            table: k / SLOTS,
            slot: k % SLOTS,
            rights,
        };
    }
    k -= GRANTS;
    Op::Revoke {
        table: k / SLOTS,
        slot: k % SLOTS,
    }
}

/// Applies `op`. Returns `None` if the operation names an empty slot, so it is
/// not a step of the model.
fn apply(state: &mut State, op: Op) -> Option<Result<(), CapError>> {
    match op {
        Op::Mint {
            table,
            object,
            rights,
        } => Some(
            state[table]
                .mint(OBJECTS[object], RIGHTS[rights])
                .map(|_| ()),
        ),
        Op::Derive {
            table,
            slot,
            rights,
        } => {
            let handle = state[table].handle_at(slot)?;
            Some(state[table].derive(handle, RIGHTS[rights]).map(|_| ()))
        }
        Op::Grant {
            table,
            slot,
            rights,
        } => {
            let handle = state[table].handle_at(slot)?;
            let source = state[table];
            let target = 1 - table;
            Some(grant(&source, table, handle, &mut state[target], RIGHTS[rights]).map(|_| ()))
        }
        Op::Revoke { table, slot } => {
            let handle = state[table].handle_at(slot)?;
            Some(revoke_across(state, table, handle))
        }
    }
}

/// The invariants that must hold after every step.
fn check(state: &State) -> Result<(), &'static str> {
    for table in state.iter() {
        for slot in table.slots.iter().filter(|s| s.live) {
            // CAP-3: a derived capability never outlives its parent, and it
            // carries no right the parent lacks (CAP-1), on the same object (CAP-6).
            if let Some(parent) = slot.parent {
                let p = table
                    .live_slot(parent)
                    .map_err(|_| "CAP-3: derived capability outlives its parent")?;
                if !p.rights.contains(slot.rights) {
                    return Err("CAP-1: derived capability has a right its parent lacks");
                }
                if p.object != slot.object {
                    return Err("CAP-6: derived capability names another object");
                }
            }
            // CAP-5, CAP-7: a copy never outlives its origin, and it carries no
            // right and names no object its origin does not.
            if let Some(origin) = slot.origin {
                let source = state
                    .get(origin.table)
                    .ok_or("CAP-5: origin names a missing table")?;
                let s = source
                    .live_slot(origin.handle)
                    .map_err(|_| "CAP-5: copy outlives its origin")?;
                if !s.rights.contains(slot.rights) {
                    return Err("CAP-7: copy has a right its origin lacks");
                }
                if s.object != slot.object {
                    return Err("CAP-6: copy names another object");
                }
            }
        }
    }
    Ok(())
}

fn same(a: &State, b: &State) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.slots == y.slots)
}

/// Handles that were live once on this path and are not live now, with the table
/// each was in. A stale handle must stay refused, even after its slot is reused.
#[derive(Clone, Copy)]
struct Graves {
    items: [Option<(usize, CapId)>; GRAVES],
    len: usize,
}

const GRAVES: usize = 32;

impl Graves {
    const fn new() -> Self {
        Self {
            items: [None; GRAVES],
            len: 0,
        }
    }

    fn record(&mut self, table: usize, handle: CapId) {
        assert!(self.len < GRAVES, "grave buffer is full: raise GRAVES");
        self.items[self.len] = Some((table, handle));
        self.len += 1;
    }
}

fn explore(state: &State, graves: &Graves, depth: usize, steps: &mut u64) {
    if depth == DEPTH {
        return;
    }
    for k in 0..OP_COUNT {
        let op = decode(k);
        let mut next = *state;
        let Some(result) = apply(&mut next, op) else {
            continue;
        };
        *steps += 1;
        // CAP-8: a refused operation changes nothing.
        if result.is_err() && !same(&next, state) {
            panic!("CAP-8: refused operation {op:?} changed the table");
        }
        if let Err(message) = check(&next) {
            panic!("{message} after {op:?}");
        }
        // CAP-4: every handle that was live before this step and is not the
        // same live capability now is stale from here on.
        let mut next_graves = *graves;
        for (t, table) in state.iter().enumerate() {
            for i in 0..SLOTS {
                if let Some(h) = table.handle_at(i) {
                    if next[t].handle_at(i) != Some(h) {
                        next_graves.record(t, h);
                    }
                }
            }
        }
        for (t, h) in next_graves.items[..next_graves.len].iter().flatten() {
            if next[*t].lookup(*h, Rights::NONE) != Err(CapError::InvalidHandle) {
                panic!("CAP-4: stale handle {h:?} in table {t} is accepted after {op:?}");
            }
        }
        explore(&next, &next_graves, depth + 1, steps);
    }
}

#[test]
fn bounded_exhaustive_capability_invariants() {
    let initial: State = [CapTable::new(), CapTable::new()];
    let mut steps = 0;
    explore(&initial, &Graves::new(), 0, &mut steps);
    // Guard against a vacuous pass: the search must reach real states.
    assert!(steps > 10_000, "only {steps} steps explored");
}

#[test]
fn derive_accepts_exactly_grantable_subsets() {
    // Every pair of rights masks, not only the ones in RIGHTS.
    for parent_bits in 0..=7u64 {
        for child_bits in 0..=7u64 {
            let parent = Rights::from_bits(parent_bits).unwrap();
            let child = Rights::from_bits(child_bits).unwrap();
            let mut table: CapTable<2> = CapTable::new();
            let handle = table.mint(ObjectId(9), parent).unwrap();
            let expected = if !parent.contains(Rights::GRANT) {
                Err(CapError::NotGrantable)
            } else if !parent.contains(child) {
                Err(CapError::Denied)
            } else {
                Ok(())
            };
            let got = table.derive(handle, child).map(|_| ());
            assert_eq!(
                got, expected,
                "parent {parent_bits:#b}, child {child_bits:#b}"
            );
        }
    }
}
