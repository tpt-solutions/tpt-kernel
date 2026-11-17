// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process lifecycle for the root server: a fixed table of process slots.
//!
//! A slot moves Loading -> Running -> Exited -> Reclaimed. Each slot has a
//! generation counter that rises when the slot is reclaimed, so a
//! [`ProcessId`] taken before reuse no longer names the new occupant. The
//! table holds no heap and does not talk to the kernel: this is the
//! bookkeeping the root server would do around a spawn.
//!
//! Slot 0 is the root server. It starts running and can neither exit nor be
//! reaped. When a process exits, its children are re-parented to slot 0 so
//! none of them dangle. Because of that, an exited process never has children,
//! so the child check in [`Lifecycle::reap`] is a guard that the public
//! operations cannot trigger.
//!
//! Open: this state machine is not wired to the kernel's process or
//! capability tables, and it does not revoke a process's handles on reap.

/// Number of process slots, including the root in slot 0.
pub const MAX_PROCESSES: usize = 8;

/// Slot index of the root server.
pub const ROOT_SLOT: u8 = 0;

/// A process named by its slot and the generation the slot had when it was
/// handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessId {
    /// Slot index, below [`MAX_PROCESSES`].
    pub slot: u8,
    /// Generation of the slot when this id was issued.
    pub generation: u32,
}

/// Where a slot is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Never used.
    Free,
    /// Spawned, image not yet mapped.
    Loading,
    /// Image mapped and running.
    Running,
    /// Exited with this code, waiting to be reaped.
    Exited(i32),
    /// Reaped. The slot is reusable and its generation has risen.
    Reclaimed,
}

/// Why a lifecycle operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    /// The slot index is outside the table.
    BadId,
    /// The id names a generation that no longer occupies the slot.
    Stale,
    /// The process is not in a state that allows this operation.
    WrongState,
    /// The parent is not running, so it cannot spawn.
    ParentNotRunning,
    /// Every slot is in use.
    TableFull,
    /// The process still has children attached to it.
    ChildrenPending,
    /// The root server cannot exit or be reaped.
    RootProtected,
}

/// One slot's record, as seen through [`Lifecycle::lookup`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Process {
    /// Current generation of the slot.
    pub generation: u32,
    /// Current state.
    pub state: State,
    /// Slot index of the parent process.
    pub parent: u8,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    generation: u32,
    state: State,
    parent: u8,
}

impl Slot {
    /// True for a slot that holds a process, whether loading, running or exited.
    fn is_occupied(&self) -> bool {
        !matches!(self.state, State::Free | State::Reclaimed)
    }
}

/// The process table.
#[derive(Debug)]
pub struct Lifecycle {
    slots: [Slot; MAX_PROCESSES],
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    /// A table with only the root server, running in slot 0.
    #[must_use]
    pub fn new() -> Self {
        let mut slots = [Slot {
            generation: 0,
            state: State::Free,
            parent: ROOT_SLOT,
        }; MAX_PROCESSES];
        slots[ROOT_SLOT as usize].state = State::Running;
        Self { slots }
    }

    /// The id of the root server.
    #[must_use]
    pub fn root(&self) -> ProcessId {
        ProcessId {
            slot: ROOT_SLOT,
            generation: self.slots[ROOT_SLOT as usize].generation,
        }
    }

    fn occupant(&self, id: ProcessId) -> Result<&Slot, LifecycleError> {
        let slot = self
            .slots
            .get(id.slot as usize)
            .ok_or(LifecycleError::BadId)?;
        if slot.generation != id.generation || !slot.is_occupied() {
            return Err(LifecycleError::Stale);
        }
        Ok(slot)
    }

    /// The record for `id`, or an error if the id is stale or unknown.
    pub fn lookup(&self, id: ProcessId) -> Result<Process, LifecycleError> {
        let s = self.occupant(id)?;
        Ok(Process {
            generation: s.generation,
            state: s.state,
            parent: s.parent,
        })
    }

    /// Starts a child of `parent`, which must be running. The child is in
    /// `Loading` until [`finish_load`](Self::finish_load).
    pub fn begin_spawn(&mut self, parent: ProcessId) -> Result<ProcessId, LifecycleError> {
        if self.occupant(parent)?.state != State::Running {
            return Err(LifecycleError::ParentNotRunning);
        }
        let index = self
            .slots
            .iter()
            .position(|s| !s.is_occupied())
            .ok_or(LifecycleError::TableFull)?;
        let slot = &mut self.slots[index];
        slot.state = State::Loading;
        slot.parent = parent.slot;
        Ok(ProcessId {
            slot: index as u8,
            generation: slot.generation,
        })
    }

    /// Marks the image as mapped: `Loading` becomes `Running`.
    pub fn finish_load(&mut self, id: ProcessId) -> Result<(), LifecycleError> {
        self.occupant(id)?;
        let slot = &mut self.slots[id.slot as usize];
        if slot.state != State::Loading {
            return Err(LifecycleError::WrongState);
        }
        slot.state = State::Running;
        Ok(())
    }

    /// Records an exit with `code`: `Running` becomes `Exited`, and the
    /// process's children are re-parented to the root.
    pub fn exit(&mut self, id: ProcessId, code: i32) -> Result<(), LifecycleError> {
        if id.slot == ROOT_SLOT {
            return Err(LifecycleError::RootProtected);
        }
        if self.occupant(id)?.state != State::Running {
            return Err(LifecycleError::WrongState);
        }
        self.slots[id.slot as usize].state = State::Exited(code);
        for child in self.slots.iter_mut() {
            if child.parent == id.slot && child.is_occupied() {
                child.parent = ROOT_SLOT;
            }
        }
        Ok(())
    }

    /// Reclaims an exited process. The slot's generation rises, so every id
    /// issued for it before now is stale. Refused for the root, for a process
    /// that has not exited, and while any child is still attached to it.
    pub fn reap(&mut self, id: ProcessId) -> Result<(), LifecycleError> {
        if id.slot == ROOT_SLOT {
            return Err(LifecycleError::RootProtected);
        }
        if !matches!(self.occupant(id)?.state, State::Exited(_)) {
            return Err(LifecycleError::WrongState);
        }
        let attached = self
            .slots
            .iter()
            .enumerate()
            .any(|(i, c)| i != id.slot as usize && c.parent == id.slot && c.is_occupied());
        if attached {
            return Err(LifecycleError::ChildrenPending);
        }
        let slot = &mut self.slots[id.slot as usize];
        slot.state = State::Reclaimed;
        slot.generation = slot.generation.wrapping_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_running(t: &mut Lifecycle, parent: ProcessId) -> ProcessId {
        let id = t.begin_spawn(parent).unwrap();
        t.finish_load(id).unwrap();
        id
    }

    #[test]
    fn happy_path_spawn_load_run_exit_reap() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let id = t.begin_spawn(root).unwrap();
        assert_eq!(t.lookup(id).unwrap().state, State::Loading);
        t.finish_load(id).unwrap();
        assert_eq!(t.lookup(id).unwrap().state, State::Running);
        t.exit(id, 7).unwrap();
        assert_eq!(t.lookup(id).unwrap().state, State::Exited(7));
        t.reap(id).unwrap();
        // After reaping, the old id no longer names the slot.
        assert_eq!(t.lookup(id), Err(LifecycleError::Stale));
    }

    #[test]
    fn stale_id_is_refused_after_slot_reuse() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let first = spawn_running(&mut t, root);
        t.exit(first, 0).unwrap();
        t.reap(first).unwrap();
        let second = spawn_running(&mut t, root);
        assert_eq!(second.slot, first.slot);
        assert_ne!(second.generation, first.generation);
        assert_eq!(t.lookup(first), Err(LifecycleError::Stale));
        assert_eq!(t.exit(first, 1), Err(LifecycleError::Stale));
        assert_eq!(t.lookup(second).unwrap().state, State::Running);
    }

    #[test]
    fn exit_of_a_stale_id_is_refused() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let id = spawn_running(&mut t, root);
        t.exit(id, 0).unwrap();
        t.reap(id).unwrap();
        assert_eq!(t.exit(id, 0), Err(LifecycleError::Stale));
    }

    #[test]
    fn exit_requires_running_and_finish_load_requires_loading() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let id = t.begin_spawn(root).unwrap();
        assert_eq!(t.exit(id, 0), Err(LifecycleError::WrongState));
        t.finish_load(id).unwrap();
        assert_eq!(t.finish_load(id), Err(LifecycleError::WrongState));
    }

    #[test]
    fn reap_requires_exited() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let id = spawn_running(&mut t, root);
        assert_eq!(t.reap(id), Err(LifecycleError::WrongState));
    }

    #[test]
    fn children_are_reparented_to_root_when_parent_exits() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let parent = spawn_running(&mut t, root);
        let child = spawn_running(&mut t, parent);
        assert_eq!(t.lookup(child).unwrap().parent, parent.slot);
        t.exit(parent, 0).unwrap();
        assert_eq!(t.lookup(child).unwrap().parent, ROOT_SLOT);
        // The parent can now be reaped, and the child still runs.
        t.reap(parent).unwrap();
        assert_eq!(t.lookup(child).unwrap().state, State::Running);
        t.exit(child, 0).unwrap();
        t.reap(child).unwrap();
    }

    #[test]
    fn exited_parent_cannot_spawn() {
        let mut t = Lifecycle::new();
        let root = t.root();
        let parent = spawn_running(&mut t, root);
        t.exit(parent, 0).unwrap();
        assert_eq!(t.begin_spawn(parent), Err(LifecycleError::ParentNotRunning));
    }

    #[test]
    fn table_full_returns_an_error() {
        let mut t = Lifecycle::new();
        let root = t.root();
        // Slot 0 is the root, so MAX_PROCESSES - 1 children fit.
        for _ in 1..MAX_PROCESSES {
            spawn_running(&mut t, root);
        }
        assert_eq!(t.begin_spawn(root), Err(LifecycleError::TableFull));
    }

    #[test]
    fn root_cannot_exit_or_be_reaped() {
        let mut t = Lifecycle::new();
        let root = t.root();
        assert_eq!(t.exit(root, 0), Err(LifecycleError::RootProtected));
        assert_eq!(t.reap(root), Err(LifecycleError::RootProtected));
        assert_eq!(t.lookup(root).unwrap().state, State::Running);
    }

    #[test]
    fn out_of_range_slot_is_a_bad_id() {
        let t = Lifecycle::new();
        let id = ProcessId {
            slot: MAX_PROCESSES as u8,
            generation: 0,
        };
        assert_eq!(t.lookup(id), Err(LifecycleError::BadId));
    }
}
