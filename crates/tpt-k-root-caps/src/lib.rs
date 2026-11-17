// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k root capabilities: generation, delegation and revocation.
//!
//! The root server holds a fixed table of capability slots. [`CapTable::mint`]
//! creates a root capability for an object. [`CapTable::delegate`] copies one
//! into a new slot with fewer rights, and records the copy as a child of the
//! original. [`CapTable::revoke`] kills a capability and every descendant of
//! it. Each slot has a generation counter that changes when the slot dies, so
//! a handle to a dead capability stays dead even after its slot is reused.
//!
//! The table holds no heap memory. Revocation scans the table until nothing
//! changes, so it costs O(N²) in the worst case for N slots.

#![no_std]

#[cfg(test)]
extern crate std;

use tpt_k_ipc_cap::{Capability, ObjectId, Rights};

/// A name for a capability slot. It is valid only while its generation matches
/// the slot's, so a stale handle is refused after revocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    index: u16,
    generation: u32,
}

impl Handle {
    /// The slot index. Useful for logging; it is not a capability by itself.
    #[must_use]
    pub const fn index(self) -> u16 {
        self.index
    }
}

/// Why a capability operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    /// Every slot is in use.
    Full,
    /// The handle names a dead or reused slot, or an index out of range.
    Stale,
    /// The capability lacks a right the operation needs.
    NoRight,
    /// A delegation asked for rights the source does not hold.
    Widen,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    live: bool,
    generation: u32,
    capability: Capability,
    /// The capability this one was delegated from. Its generation tells whether
    /// the parent is still the same live capability.
    parent: Option<Handle>,
}

const EMPTY_SLOT: Slot = Slot {
    live: false,
    generation: 0,
    capability: Capability {
        object: ObjectId(0),
        rights: Rights::NONE,
    },
    parent: None,
};

/// A fixed table of `N` capability slots.
#[derive(Debug)]
pub struct CapTable<const N: usize> {
    slots: [Slot; N],
}

impl<const N: usize> Default for CapTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CapTable<N> {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [EMPTY_SLOT; N],
        }
    }

    /// Number of live capabilities.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }

    fn insert(
        &mut self,
        capability: Capability,
        parent: Option<Handle>,
    ) -> Result<Handle, CapError> {
        let index = self
            .slots
            .iter()
            .position(|s| !s.live)
            .ok_or(CapError::Full)?;
        let slot = &mut self.slots[index];
        slot.live = true;
        slot.capability = capability;
        slot.parent = parent;
        Ok(Handle {
            index: index as u16,
            generation: slot.generation,
        })
    }

    /// Creates a root capability for `object` with `rights`.
    pub fn mint(&mut self, object: ObjectId, rights: Rights) -> Result<Handle, CapError> {
        self.insert(Capability { object, rights }, None)
    }

    /// The capability behind a handle, if the handle is still live.
    pub fn lookup(&self, handle: Handle) -> Result<Capability, CapError> {
        let slot = self
            .slots
            .get(handle.index as usize)
            .ok_or(CapError::Stale)?;
        if slot.live && slot.generation == handle.generation {
            Ok(slot.capability)
        } else {
            Err(CapError::Stale)
        }
    }

    /// The capability behind a handle, if it is live and carries `needed`.
    pub fn check(&self, handle: Handle, needed: Rights) -> Result<Capability, CapError> {
        let cap = self.lookup(handle)?;
        if cap.allows(needed) {
            Ok(cap)
        } else {
            Err(CapError::NoRight)
        }
    }

    /// Copies the capability behind `from` into a new slot with `rights`, which
    /// must be a subset of what `from` holds. The copy is a child of `from`.
    /// The caller needs `GRANT` on `from`.
    pub fn delegate(&mut self, from: Handle, rights: Rights) -> Result<Handle, CapError> {
        let source = self.check(from, Rights::GRANT)?;
        let narrowed = source.attenuate(rights).ok_or(CapError::Widen)?;
        self.insert(narrowed, Some(from))
    }

    /// Kills the capability behind `handle` and every capability derived from
    /// it, directly or through other copies. Siblings and parents are left
    /// alone. The caller needs `REVOKE` on `handle`.
    ///
    /// Returns how many capabilities died, including `handle` itself.
    pub fn revoke(&mut self, handle: Handle) -> Result<usize, CapError> {
        self.check(handle, Rights::REVOKE)?;
        let mut killed = self.kill(handle.index as usize);
        loop {
            let mut changed = false;
            for i in 0..N {
                let orphaned = {
                    let s = &self.slots[i];
                    s.live && s.parent.is_some_and(|p| self.lookup(p).is_err())
                };
                if orphaned {
                    killed += self.kill(i);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(killed)
    }

    /// Marks one slot dead and bumps its generation. Returns 1 if it was live.
    fn kill(&mut self, index: usize) -> usize {
        let slot = &mut self.slots[index];
        if !slot.live {
            return 0;
        }
        slot.live = false;
        slot.generation = slot.generation.wrapping_add(1);
        slot.parent = None;
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: ObjectId = ObjectId(7);

    #[test]
    fn mint_and_lookup() {
        let mut t: CapTable<4> = CapTable::new();
        let h = t.mint(OBJ, Rights::SEND).unwrap();
        assert_eq!(t.lookup(h).unwrap().object, OBJ);
        assert_eq!(t.live_count(), 1);
    }

    #[test]
    fn table_reports_full() {
        let mut t: CapTable<2> = CapTable::new();
        t.mint(OBJ, Rights::SEND).unwrap();
        t.mint(OBJ, Rights::SEND).unwrap();
        assert_eq!(t.mint(OBJ, Rights::SEND), Err(CapError::Full));
    }

    #[test]
    fn delegate_narrows_and_refuses_widening() {
        let mut t: CapTable<8> = CapTable::new();
        let root = t.mint(OBJ, Rights::ALL).unwrap();
        let child = t.delegate(root, Rights::RECV).unwrap();
        assert_eq!(t.lookup(child).unwrap().rights, Rights::RECV);
        // A child with only RECV has no GRANT, so it cannot delegate further.
        assert_eq!(t.delegate(child, Rights::RECV), Err(CapError::NoRight));
        // A GRANT holder asking for a right it lacks is a widening attempt.
        let g = t.delegate(root, Rights::GRANT.union(Rights::SEND)).unwrap();
        assert_eq!(t.delegate(g, Rights::MAP), Err(CapError::Widen));
    }

    #[test]
    fn delegate_needs_grant() {
        let mut t: CapTable<4> = CapTable::new();
        let h = t.mint(OBJ, Rights::SEND.union(Rights::RECV)).unwrap();
        assert_eq!(t.delegate(h, Rights::SEND), Err(CapError::NoRight));
    }

    #[test]
    fn revoke_cascades_to_children_and_grandchildren() {
        let mut t: CapTable<8> = CapTable::new();
        let root = t.mint(OBJ, Rights::ALL).unwrap();
        let child = t.delegate(root, Rights::ALL).unwrap();
        let grandchild = t
            .delegate(child, Rights::SEND.union(Rights::GRANT))
            .unwrap();
        let great = t.delegate(grandchild, Rights::SEND).unwrap();
        assert_eq!(t.revoke(child), Ok(3));
        assert_eq!(t.lookup(child), Err(CapError::Stale));
        assert_eq!(t.lookup(grandchild), Err(CapError::Stale));
        assert_eq!(t.lookup(great), Err(CapError::Stale));
        assert!(t.lookup(root).is_ok());
    }

    #[test]
    fn revoke_leaves_parents_and_siblings_alone() {
        let mut t: CapTable<8> = CapTable::new();
        let root = t.mint(OBJ, Rights::ALL).unwrap();
        let a = t.delegate(root, Rights::ALL).unwrap();
        let b = t.delegate(root, Rights::ALL).unwrap();
        let a_child = t.delegate(a, Rights::SEND).unwrap();
        assert_eq!(t.revoke(a), Ok(2));
        assert!(t.lookup(root).is_ok());
        assert!(t.lookup(b).is_ok());
        assert_eq!(t.lookup(a_child), Err(CapError::Stale));
    }

    #[test]
    fn revoke_needs_revoke_right() {
        let mut t: CapTable<4> = CapTable::new();
        let rights = Rights::REVOKE.union(Rights::GRANT).union(Rights::SEND);
        let h = t.mint(OBJ, rights).unwrap();
        // Delegated copy without REVOKE cannot revoke itself.
        let no_revoke = t.delegate(h, Rights::SEND).unwrap();
        assert_eq!(t.revoke(no_revoke), Err(CapError::NoRight));
        assert!(t.lookup(no_revoke).is_ok());
        // A root without REVOKE cannot revoke either.
        let plain = t.mint(OBJ, Rights::SEND).unwrap();
        assert_eq!(t.revoke(plain), Err(CapError::NoRight));
    }

    #[test]
    fn reused_slot_does_not_resurrect_old_handle() {
        let mut t: CapTable<1> = CapTable::new();
        let old = t.mint(OBJ, Rights::SEND.union(Rights::REVOKE)).unwrap();
        assert_eq!(t.revoke(old), Ok(1));
        let new = t.mint(ObjectId(8), Rights::RECV).unwrap();
        assert_eq!(new.index(), old.index());
        assert_eq!(t.lookup(old), Err(CapError::Stale));
        assert_eq!(t.check(old, Rights::NONE), Err(CapError::Stale));
        assert_eq!(t.lookup(new).unwrap().object, ObjectId(8));
    }

    #[test]
    fn out_of_range_handle_is_stale() {
        let mut t: CapTable<2> = CapTable::new();
        let h = t.mint(OBJ, Rights::SEND).unwrap();
        let big = Handle {
            index: 3,
            generation: h.generation,
        };
        assert_eq!(t.lookup(big), Err(CapError::Stale));
    }

    #[test]
    fn check_reports_missing_rights() {
        let mut t: CapTable<2> = CapTable::new();
        let h = t.mint(OBJ, Rights::SEND).unwrap();
        assert!(t.check(h, Rights::SEND).is_ok());
        assert_eq!(t.check(h, Rights::MAP), Err(CapError::NoRight));
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use std::vec::Vec;

    /// Deterministic xorshift64 generator. Seeded with a constant, so every run
    /// checks the same cases. This is property-style testing, not
    /// coverage-guided fuzzing.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn u32(&mut self) -> u32 {
            self.next() as u32
        }

        /// A value in `0..n`, for `n > 0`.
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Cases per property. Enough to cover the input space in practice, and
    /// small enough that the suite stays fast in debug builds.
    const CASES: usize = 512;

    #[derive(Clone, Copy, Debug)]
    enum Op {
        Mint(u64, u32),
        Delegate(usize, u32),
        Revoke(usize),
    }

    fn op(rng: &mut Rng) -> Op {
        match rng.below(3) {
            0 => Op::Mint(rng.below(4) as u64, rng.u32()),
            1 => Op::Delegate(rng.next() as usize, rng.u32()),
            _ => Op::Revoke(rng.next() as usize),
        }
    }

    /// Checks the tree invariants over every live slot.
    fn check_tree<const N: usize>(t: &CapTable<N>) {
        for (i, s) in t.slots.iter().enumerate() {
            if !s.live {
                continue;
            }
            if let Some(p) = s.parent {
                let ps = &t.slots[p.index as usize];
                // Invariant: a live capability's parent is live (revocation
                // cascades), and it is the generation the child recorded.
                assert!(
                    ps.live && ps.generation == p.generation,
                    "live slot {} has a dead parent",
                    i
                );
                // Invariant: a derived capability never holds rights its parent lacks.
                assert!(
                    ps.capability.rights.contains(s.capability.rights),
                    "slot {} has rights its parent lacks",
                    i
                );
                assert_eq!(ps.capability.object, s.capability.object);
            }
        }
    }

    /// True if `h` is on the parent chain of any live slot.
    fn has_live_descendant_of<const N: usize>(t: &CapTable<N>, h: Handle) -> bool {
        t.slots.iter().any(|s| {
            if !s.live {
                return false;
            }
            let mut cur = s.parent;
            let mut steps = 0;
            while let Some(p) = cur {
                if p == h {
                    return true;
                }
                steps += 1;
                if steps > N {
                    break;
                }
                cur = t.slots[p.index as usize].parent;
            }
            false
        })
    }

    /// Invariant: no capability widening and no dangling derivation across
    /// random mint, delegate and revoke sequences. Revoking a capability leaves
    /// no live descendant.
    #[test]
    fn capability_tree_invariants_hold_over_random_operations() {
        let mut rng = Rng(0x94D0_49BB_1331_11EB);
        for _ in 0..CASES {
            let len = rng.below(40);
            let ops: Vec<Op> = (0..len).map(|_| op(&mut rng)).collect();
            let mut t: CapTable<8> = CapTable::new();
            let mut issued: Vec<Handle> = Vec::new();
            for o in ops {
                match o {
                    Op::Mint(obj, r) => {
                        if let Ok(h) = t.mint(ObjectId(obj), Rights::from_bits(r)) {
                            issued.push(h);
                        }
                    }
                    Op::Delegate(i, r) => {
                        if !issued.is_empty() {
                            let h = issued[i % issued.len()];
                            if let Ok(c) = t.delegate(h, Rights::from_bits(r)) {
                                issued.push(c);
                            }
                        }
                    }
                    Op::Revoke(i) => {
                        if !issued.is_empty() {
                            let h = issued[i % issued.len()];
                            if t.revoke(h).is_ok() {
                                assert_eq!(t.lookup(h), Err(CapError::Stale));
                                assert!(!has_live_descendant_of(&t, h));
                            }
                        }
                    }
                }
                check_tree(&t);
            }
        }
    }
}
