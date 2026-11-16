// SPDX-License-Identifier: MIT OR Apache-2.0
//! The capability table. A capability names a kernel object and the rights its
//! holder has on it. Handles carry a generation, so a handle to a revoked slot
//! fails even after the slot is reused. Derivation can only remove rights, and
//! revoking a capability revokes every capability derived from it.

/// A kernel object, named by the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u32);

/// A set of rights on an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rights(u8);

impl Rights {
    /// No rights.
    pub const NONE: Self = Self(0);
    /// Receive from an endpoint, or read from a memory object.
    pub const READ: Self = Self(1);
    /// Send to an endpoint, or write to a memory object.
    pub const WRITE: Self = Self(2);
    /// Derive capabilities from this one.
    pub const GRANT: Self = Self(4);
    /// Every right.
    pub const ALL: Self = Self(7);

    /// True if every right in `other` is also in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Rights from a raw mask. Returns `None` if the mask names a bit that is not
    /// a right.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Option<Self> {
        if bits & !(Self::ALL.0 as u64) == 0 {
            Some(Self(bits as u8))
        } else {
            None
        }
    }

    /// The rights in either set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A handle to a capability: a slot index and the generation the slot had when
/// the capability was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapId {
    index: u32,
    generation: u32,
}

impl CapId {
    /// The slot index. Meaningful only for a live handle.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index as usize
    }

    /// The handle as one word, for passing through a register. The generation is
    /// in the high half and the index in the low half.
    #[must_use]
    pub const fn raw(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    /// Rebuilds a handle from [`CapId::raw`]. Any word is accepted: a forged
    /// value names no live slot, and lookups refuse it.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self {
            index: raw as u32,
            generation: (raw >> 32) as u32,
        }
    }
}

/// Why a capability operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    /// Every slot is in use.
    Full,
    /// The handle is stale or was never valid.
    InvalidHandle,
    /// The capability lacks a needed right, or a derivation asks for a right the
    /// parent does not have.
    Denied,
    /// The parent does not carry `GRANT`, so it cannot be derived from.
    NotGrantable,
}

/// Where a capability was copied from, when it came from another table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    /// Index of the source table in the set of tables.
    pub table: usize,
    /// The source handle, valid in that table.
    pub handle: CapId,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    live: bool,
    generation: u32,
    object: ObjectId,
    rights: Rights,
    parent: Option<CapId>,
    origin: Option<Origin>,
}

const EMPTY: Slot = Slot {
    live: false,
    generation: 0,
    object: ObjectId(0),
    rights: Rights::NONE,
    parent: None,
    origin: None,
};

/// A fixed table of `N` capability slots.
#[derive(Clone, Copy, Debug)]
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
        Self { slots: [EMPTY; N] }
    }

    /// Number of live capabilities.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }

    /// Creates a root capability: one with no parent.
    ///
    /// # Errors
    ///
    /// [`CapError::Full`] if no slot is free.
    pub fn mint(&mut self, object: ObjectId, rights: Rights) -> Result<CapId, CapError> {
        self.insert(object, rights, None)
    }

    /// Checks a handle and the rights it needs, and returns the object it names.
    ///
    /// # Errors
    ///
    /// [`CapError::InvalidHandle`] for a stale handle, and [`CapError::Denied`]
    /// if the capability lacks `need`.
    pub fn lookup(&self, id: CapId, need: Rights) -> Result<ObjectId, CapError> {
        let slot = self.live_slot(id)?;
        if slot.rights.contains(need) {
            Ok(slot.object)
        } else {
            Err(CapError::Denied)
        }
    }

    /// The rights a live capability carries.
    ///
    /// # Errors
    ///
    /// [`CapError::InvalidHandle`] for a stale handle.
    pub fn rights_of(&self, id: CapId) -> Result<Rights, CapError> {
        Ok(self.live_slot(id)?.rights)
    }

    /// Creates a child capability with `rights`, which must be a subset of the
    /// parent's. The parent must carry `GRANT`.
    ///
    /// # Errors
    ///
    /// [`CapError::InvalidHandle`] for a stale parent,
    /// [`CapError::NotGrantable`] if the parent lacks `GRANT`,
    /// [`CapError::Denied`] if `rights` is wider than the parent's, and
    /// [`CapError::Full`] if no slot is free.
    pub fn derive(&mut self, parent: CapId, rights: Rights) -> Result<CapId, CapError> {
        let slot = *self.live_slot(parent)?;
        if !slot.rights.contains(Rights::GRANT) {
            return Err(CapError::NotGrantable);
        }
        if !slot.rights.contains(rights) {
            return Err(CapError::Denied);
        }
        self.insert(slot.object, rights, Some(parent))
    }

    /// Revokes a capability and every capability derived from it, directly or
    /// through a chain of derivations. Their handles become invalid.
    ///
    /// # Errors
    ///
    /// [`CapError::InvalidHandle`] if the handle is already stale.
    pub fn revoke(&mut self, id: CapId) -> Result<(), CapError> {
        self.live_slot(id)?;
        self.free(id.index());
        // Each pass frees children of freed parents. A pass that frees nothing
        // ends the search, and there are at most N passes.
        loop {
            let mut freed = false;
            for index in 0..N {
                let slot = self.slots[index];
                if let (true, Some(parent)) = (slot.live, slot.parent) {
                    if self.live_slot(parent).is_err() {
                        self.free(index);
                        freed = true;
                    }
                }
            }
            if !freed {
                return Ok(());
            }
        }
    }

    fn insert(
        &mut self,
        object: ObjectId,
        rights: Rights,
        parent: Option<CapId>,
    ) -> Result<CapId, CapError> {
        let index = self
            .slots
            .iter()
            .position(|s| !s.live)
            .ok_or(CapError::Full)?;
        let slot = &mut self.slots[index];
        slot.live = true;
        slot.object = object;
        slot.rights = rights;
        slot.parent = parent;
        Ok(CapId {
            index: index as u32,
            generation: slot.generation,
        })
    }

    fn free(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        slot.live = false;
        slot.generation = slot.generation.wrapping_add(1);
        slot.parent = None;
        slot.origin = None;
    }

    /// True if `id` names a live capability in this table.
    #[must_use]
    pub fn is_live(&self, id: CapId) -> bool {
        self.live_slot(id).is_ok()
    }

    /// The handle of the live capability in slot `index`, if there is one.
    #[must_use]
    pub fn handle_at(&self, index: usize) -> Option<CapId> {
        let slot = self.slots.get(index)?;
        slot.live.then_some(CapId {
            index: index as u32,
            generation: slot.generation,
        })
    }

    /// The origin of the live capability in slot `index`, if it was copied from
    /// another table.
    #[must_use]
    pub fn origin_at(&self, index: usize) -> Option<Origin> {
        let slot = self.slots.get(index)?;
        if slot.live {
            slot.origin
        } else {
            None
        }
    }

    /// Creates a capability that records where it was copied from. Used when a
    /// capability is granted from another table.
    ///
    /// # Errors
    ///
    /// [`CapError::Full`] if no slot is free.
    pub fn mint_granted(
        &mut self,
        object: ObjectId,
        rights: Rights,
        origin: Origin,
    ) -> Result<CapId, CapError> {
        let id = self.insert(object, rights, None)?;
        self.slots[id.index()].origin = Some(origin);
        Ok(id)
    }

    fn live_slot(&self, id: CapId) -> Result<&Slot, CapError> {
        let slot = self.slots.get(id.index()).ok_or(CapError::InvalidHandle)?;
        if slot.live && slot.generation == id.generation {
            Ok(slot)
        } else {
            Err(CapError::InvalidHandle)
        }
    }
}

/// Copies a capability from `source` into `target`, with `rights`. The source
/// capability must carry `GRANT`, and `rights` must be a subset of its rights.
/// The new capability names the same object. The copy records its origin, so
/// [`revoke_across`] can revoke it when the source is revoked.
///
/// `source_table` is the index of `source` in the set of tables that
/// `revoke_across` works on.
///
/// # Errors
///
/// [`CapError::InvalidHandle`] for a stale source handle,
/// [`CapError::NotGrantable`] if the source lacks `GRANT`, [`CapError::Denied`] if
/// `rights` is wider than the source's, and [`CapError::Full`] if the target has
/// no free slot.
pub fn grant<const M: usize, const N: usize>(
    source: &CapTable<M>,
    source_table: usize,
    handle: CapId,
    target: &mut CapTable<N>,
    rights: Rights,
) -> Result<CapId, CapError> {
    let held = source.rights_of(handle)?;
    if !held.contains(Rights::GRANT) {
        return Err(CapError::NotGrantable);
    }
    if !held.contains(rights) {
        return Err(CapError::Denied);
    }
    let object = source.lookup(handle, Rights::NONE)?;
    let origin = Origin {
        table: source_table,
        handle,
    };
    target.mint_granted(object, rights, origin)
}

/// Revokes capability `id` of table `table`, the capabilities derived from it in
/// that table, and every copy made from any of these, in any table, transitively.
///
/// A copy is revoked when its origin is no longer live. The scan repeats until a
/// pass revokes nothing. Each pass that revokes something removes at least one
/// capability, so the number of passes is bounded by the total slot count.
///
/// # Errors
///
/// [`CapError::InvalidHandle`] if `id` is not live in `table`.
pub fn revoke_across<const N: usize>(
    tables: &mut [CapTable<N>],
    table: usize,
    id: CapId,
) -> Result<(), CapError> {
    tables
        .get_mut(table)
        .ok_or(CapError::InvalidHandle)?
        .revoke(id)?;
    loop {
        let mut changed = false;
        for holder in 0..tables.len() {
            for index in 0..N {
                let Some(origin) = tables[holder].origin_at(index) else {
                    continue;
                };
                let source_live = tables
                    .get(origin.table)
                    .is_some_and(|source| source.is_live(origin.handle));
                if source_live {
                    continue;
                }
                if let Some(handle) = tables[holder].handle_at(index) {
                    tables[holder].revoke(handle)?;
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_copies_a_subset_of_rights_into_another_table() {
        let mut owner: CapTable<4> = CapTable::new();
        let mut other: CapTable<4> = CapTable::new();
        let root = owner.mint(ObjectId(5), Rights::ALL).unwrap();
        let copy = grant(&owner, 0, root, &mut other, Rights::READ).unwrap();
        assert_eq!(other.lookup(copy, Rights::READ), Ok(ObjectId(5)));
        assert_eq!(other.lookup(copy, Rights::WRITE), Err(CapError::Denied));
        // The owner's table is unchanged, and the copy is not a handle there.
        assert_eq!(owner.live_count(), 1);
    }

    #[test]
    fn grant_needs_grant_and_refuses_widening() {
        let mut owner: CapTable<4> = CapTable::new();
        let mut other: CapTable<4> = CapTable::new();
        let no_grant = owner.mint(ObjectId(1), Rights::READ).unwrap();
        assert_eq!(
            grant(&owner, 0, no_grant, &mut other, Rights::READ),
            Err(CapError::NotGrantable)
        );
        let root = owner
            .mint(ObjectId(1), Rights::READ.union(Rights::GRANT))
            .unwrap();
        assert_eq!(
            grant(&owner, 0, root, &mut other, Rights::WRITE),
            Err(CapError::Denied)
        );
        assert_eq!(other.live_count(), 0);
    }

    #[test]
    fn grant_reports_a_full_target() {
        let mut owner: CapTable<4> = CapTable::new();
        let mut full: CapTable<1> = CapTable::new();
        full.mint(ObjectId(9), Rights::READ).unwrap();
        let root = owner.mint(ObjectId(1), Rights::ALL).unwrap();
        assert_eq!(
            grant(&owner, 0, root, &mut full, Rights::READ),
            Err(CapError::Full)
        );
    }

    const OBJ: ObjectId = ObjectId(7);

    #[test]
    fn minted_capability_checks_its_rights() {
        let mut table: CapTable<4> = CapTable::new();
        let cap = table.mint(OBJ, Rights::READ).unwrap();
        assert_eq!(table.lookup(cap, Rights::READ), Ok(OBJ));
        assert_eq!(table.lookup(cap, Rights::WRITE), Err(CapError::Denied));
        assert_eq!(table.live_count(), 1);
    }

    #[test]
    fn derive_can_only_narrow_and_needs_grant() {
        let mut table: CapTable<4> = CapTable::new();
        let root = table.mint(OBJ, Rights::READ.union(Rights::GRANT)).unwrap();
        let child = table.derive(root, Rights::READ).unwrap();
        assert_eq!(table.lookup(child, Rights::WRITE), Err(CapError::Denied));
        assert_eq!(table.derive(root, Rights::WRITE), Err(CapError::Denied));

        let leaf = table.derive(root, Rights::READ).unwrap();
        assert_eq!(
            table.derive(leaf, Rights::READ),
            Err(CapError::NotGrantable)
        );
    }

    #[test]
    fn revoke_invalidates_the_handle_even_after_slot_reuse() {
        let mut table: CapTable<1> = CapTable::new();
        let first = table.mint(OBJ, Rights::ALL).unwrap();
        table.revoke(first).unwrap();
        assert_eq!(
            table.lookup(first, Rights::READ),
            Err(CapError::InvalidHandle)
        );
        let second = table.mint(ObjectId(9), Rights::READ).unwrap();
        assert_eq!(second.index(), first.index());
        assert_eq!(
            table.lookup(first, Rights::READ),
            Err(CapError::InvalidHandle)
        );
        assert_eq!(table.lookup(second, Rights::READ), Ok(ObjectId(9)));
    }

    #[test]
    fn revoking_a_parent_revokes_the_whole_derivation_tree() {
        let mut table: CapTable<8> = CapTable::new();
        let root = table.mint(OBJ, Rights::ALL).unwrap();
        let child = table.derive(root, Rights::ALL).unwrap();
        let grandchild = table.derive(child, Rights::READ).unwrap();
        let unrelated = table.mint(ObjectId(2), Rights::READ).unwrap();
        table.revoke(child).unwrap();
        assert_eq!(
            table.lookup(grandchild, Rights::READ),
            Err(CapError::InvalidHandle)
        );
        assert_eq!(table.lookup(root, Rights::READ), Ok(OBJ));
        assert_eq!(table.lookup(unrelated, Rights::READ), Ok(ObjectId(2)));
        assert_eq!(table.live_count(), 2);
    }

    #[test]
    fn table_full_and_double_revoke_are_refused() {
        let mut table: CapTable<2> = CapTable::new();
        let a = table.mint(OBJ, Rights::READ).unwrap();
        table.mint(OBJ, Rights::READ).unwrap();
        assert_eq!(table.mint(OBJ, Rights::READ), Err(CapError::Full));
        table.revoke(a).unwrap();
        assert_eq!(table.revoke(a), Err(CapError::InvalidHandle));
    }

    #[test]
    fn raw_handles_round_trip_and_forgeries_are_refused() {
        let mut table: CapTable<2> = CapTable::new();
        let cap = table.mint(OBJ, Rights::READ).unwrap();
        assert_eq!(CapId::from_raw(cap.raw()), cap);
        assert_eq!(
            table.lookup(CapId::from_raw(cap.raw() + 1), Rights::READ),
            Err(CapError::InvalidHandle)
        );
        assert_eq!(
            table.lookup(CapId::from_raw(u64::MAX), Rights::READ),
            Err(CapError::InvalidHandle)
        );
    }

    #[test]
    fn out_of_range_handles_are_invalid() {
        let table: CapTable<2> = CapTable::new();
        let forged = CapId {
            index: 9,
            generation: 0,
        };
        assert_eq!(
            table.lookup(forged, Rights::NONE),
            Err(CapError::InvalidHandle)
        );
    }

    #[test]
    fn revoking_the_source_revokes_copies_in_other_tables_transitively() {
        let mut tables: [CapTable<4>; 3] = [CapTable::new(); 3];
        let root = tables[0].mint(ObjectId(7), Rights::ALL).unwrap();
        let copy = {
            let source = tables[0];
            grant(&source, 0, root, &mut tables[1], Rights::ALL).unwrap()
        };
        let second_copy = {
            let source = tables[1];
            grant(&source, 1, copy, &mut tables[2], Rights::READ).unwrap()
        };
        let unrelated = tables[2].mint(ObjectId(8), Rights::READ).unwrap();

        revoke_across(&mut tables, 0, root).unwrap();

        assert_eq!(
            tables[1].lookup(copy, Rights::READ),
            Err(CapError::InvalidHandle)
        );
        assert_eq!(
            tables[2].lookup(second_copy, Rights::READ),
            Err(CapError::InvalidHandle)
        );
        assert_eq!(tables[2].lookup(unrelated, Rights::READ), Ok(ObjectId(8)));
    }

    #[test]
    fn revoking_a_copy_leaves_the_source_alone() {
        let mut tables: [CapTable<4>; 2] = [CapTable::new(); 2];
        let root = tables[0].mint(ObjectId(3), Rights::ALL).unwrap();
        let copy = {
            let source = tables[0];
            grant(&source, 0, root, &mut tables[1], Rights::READ).unwrap()
        };
        revoke_across(&mut tables, 1, copy).unwrap();
        assert_eq!(tables[0].lookup(root, Rights::READ), Ok(ObjectId(3)));
        assert_eq!(
            revoke_across(&mut tables, 1, copy),
            Err(CapError::InvalidHandle)
        );
    }
}
