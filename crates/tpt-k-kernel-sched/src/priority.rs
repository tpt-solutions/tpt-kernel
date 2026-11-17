// SPDX-License-Identifier: MIT OR Apache-2.0
//! Preemptive priority scheduling core: 32 levels, round-robin within a level,
//! and time slices counted in timer ticks.
//!
//! This module is pure bookkeeping over thread slot numbers. It does not save
//! or restore registers, so it is not wired to the context switch in this
//! crate yet. The caller owns the thread table and calls [`PrioritySched::on_tick`]
//! from the timer interrupt and [`PrioritySched::reschedule`] to choose the
//! thread to run next.
//!
//! Level 0 is the highest priority. Each level is a fixed-capacity FIFO, so
//! a thread that uses up its slice goes to the back of its level and the
//! other threads at that level run first.
//!
//! Time slices get longer as priority falls: levels 0-7 get 2 ticks, 8-15 get
//! 4, 16-23 get 8, and 24-31 get 16. A higher level therefore never waits longer
//! for the CPU than a lower one would, and it is preempted more often.
//!
//! Priority inheritance is not implemented. A high-priority thread that waits
//! on a low-priority lock is not boosted, so priority inversion is possible.

/// Number of priority levels. Level 0 is the highest.
pub const LEVELS: usize = 32;

/// Why a scheduler operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedError {
    /// The priority is 32 or more.
    BadPriority,
    /// The slot number is outside the table.
    BadSlot,
    /// The slot is already registered.
    AlreadyRegistered,
    /// The slot is not registered.
    Unknown,
    /// The slot is registered but not blocked, so it cannot be woken.
    NotBlocked,
    /// The level's queue has no room. Nothing was overwritten.
    Full,
}

/// What the scheduler knows about a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// Not registered.
    Free,
    /// Waiting in its level's queue.
    Ready,
    /// The thread that holds the CPU.
    Running,
    /// Waiting for an event. It is in no queue.
    Blocked,
}

/// Ticks of CPU time a thread at `priority` receives per slice. Priorities
/// above 31 get the lowest level's quantum.
#[must_use]
pub const fn quantum(priority: u8) -> u32 {
    match priority / 8 {
        0 => 2,
        1 => 4,
        2 => 8,
        _ => 16,
    }
}

/// A fixed-capacity FIFO of slot numbers.
#[derive(Clone, Copy, Debug)]
struct Fifo<const N: usize> {
    buf: [usize; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Fifo<N> {
    const fn new() -> Self {
        Self {
            buf: [0; N],
            head: 0,
            len: 0,
        }
    }

    fn push_back(&mut self, slot: usize) -> Result<(), SchedError> {
        if self.len == N {
            return Err(SchedError::Full);
        }
        self.buf[(self.head + self.len) % N] = slot;
        self.len += 1;
        Ok(())
    }

    fn pop_front(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let slot = self.buf[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(slot)
    }

    fn front(&self) -> Option<usize> {
        (self.len > 0).then(|| self.buf[self.head])
    }

    /// Removes `slot` wherever it sits, keeping the order of the rest.
    fn remove(&mut self, slot: usize) -> bool {
        let Some(pos) = (0..self.len).find(|&i| self.buf[(self.head + i) % N] == slot) else {
            return false;
        };
        for i in pos..self.len - 1 {
            let from = (self.head + i + 1) % N;
            let to = (self.head + i) % N;
            self.buf[to] = self.buf[from];
        }
        self.len -= 1;
        true
    }
}

/// Priority scheduler over up to `SLOTS` thread slots.
///
/// Every slot is in at most one place: a level queue, the running slot, or
/// nowhere (blocked or free). Each level can hold `SLOTS` entries, so an
/// enqueue can only fail with [`SchedError::Full`] if a caller tries to add
/// a slot that is already queued, which is refused earlier as
/// `AlreadyRegistered`.
#[derive(Clone, Debug)]
pub struct PrioritySched<const SLOTS: usize> {
    queues: [Fifo<SLOTS>; LEVELS],
    state: [SlotState; SLOTS],
    priority: [u8; SLOTS],
    running: Option<usize>,
    budget: u32,
}

impl<const SLOTS: usize> Default for PrioritySched<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> PrioritySched<SLOTS> {
    /// An empty scheduler with every slot free.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            queues: [Fifo::new(); LEVELS],
            state: [SlotState::Free; SLOTS],
            priority: [0; SLOTS],
            running: None,
            budget: 0,
        }
    }

    /// The slot holding the CPU, if any.
    #[must_use]
    pub fn running(&self) -> Option<usize> {
        self.running
    }

    /// The state of `slot`, or `None` if the slot number is out of range.
    #[must_use]
    pub fn state(&self, slot: usize) -> Option<SlotState> {
        self.state.get(slot).copied()
    }

    /// The priority of `slot`, if it is registered.
    #[must_use]
    pub fn priority(&self, slot: usize) -> Option<u8> {
        match self.state.get(slot)? {
            SlotState::Free => None,
            _ => Some(self.priority[slot]),
        }
    }

    /// Registers `slot` at `priority` and queues it as ready at the back of
    /// its level.
    pub fn add(&mut self, slot: usize, priority: u8) -> Result<(), SchedError> {
        if (priority as usize) >= LEVELS {
            return Err(SchedError::BadPriority);
        }
        let st = self.state.get(slot).ok_or(SchedError::BadSlot)?;
        if *st != SlotState::Free {
            return Err(SchedError::AlreadyRegistered);
        }
        self.queues[priority as usize].push_back(slot)?;
        self.state[slot] = SlotState::Ready;
        self.priority[slot] = priority;
        Ok(())
    }

    /// Unregisters `slot` whatever its state. A running slot gives up the CPU.
    pub fn exit(&mut self, slot: usize) -> Result<(), SchedError> {
        match self.state(slot).ok_or(SchedError::BadSlot)? {
            SlotState::Free => return Err(SchedError::Unknown),
            SlotState::Ready => {
                self.queues[self.priority[slot] as usize].remove(slot);
            }
            SlotState::Running => {
                self.running = None;
                self.budget = 0;
            }
            SlotState::Blocked => {}
        }
        self.state[slot] = SlotState::Free;
        Ok(())
    }

    /// Takes the running slot off the CPU and marks it blocked. Returns the
    /// slot, or `None` if nothing was running.
    pub fn block_running(&mut self) -> Option<usize> {
        let slot = self.running.take()?;
        self.budget = 0;
        self.state[slot] = SlotState::Blocked;
        Some(slot)
    }

    /// Blocks a ready slot so it leaves its queue. A running slot is blocked
    /// too and gives up the CPU.
    pub fn block(&mut self, slot: usize) -> Result<(), SchedError> {
        match self.state(slot).ok_or(SchedError::BadSlot)? {
            SlotState::Free => Err(SchedError::Unknown),
            SlotState::Blocked => Ok(()),
            SlotState::Ready => {
                self.queues[self.priority[slot] as usize].remove(slot);
                self.state[slot] = SlotState::Blocked;
                Ok(())
            }
            SlotState::Running => {
                self.block_running();
                Ok(())
            }
        }
    }

    /// Makes a blocked slot ready again, at the back of its own level.
    pub fn unblock(&mut self, slot: usize) -> Result<(), SchedError> {
        match self.state(slot).ok_or(SchedError::BadSlot)? {
            SlotState::Free => Err(SchedError::Unknown),
            SlotState::Blocked => {
                let level = self.priority[slot] as usize;
                self.queues[level].push_back(slot)?;
                self.state[slot] = SlotState::Ready;
                Ok(())
            }
            _ => Err(SchedError::NotBlocked),
        }
    }

    /// The highest level that has a ready thread, if any.
    fn best_ready_level(&self) -> Option<usize> {
        self.queues.iter().position(|q| q.front().is_some())
    }

    /// Whether a ready thread outranks the running one, or nothing runs.
    fn higher_ready(&self) -> bool {
        match (self.running, self.best_ready_level()) {
            (None, Some(_)) => true,
            (Some(r), Some(level)) => (level as u8) < self.priority[r],
            _ => false,
        }
    }

    /// Advances the running thread's slice by one tick. Returns `true` when
    /// the caller should call [`reschedule`](Self::reschedule): the slice has
    /// run out, or a ready thread outranks the running one.
    pub fn on_tick(&mut self) -> bool {
        if self.running.is_none() {
            return self.best_ready_level().is_some();
        }
        self.budget = self.budget.saturating_sub(1);
        self.budget == 0 || self.higher_ready()
    }

    /// Chooses the slot to run next and returns it.
    ///
    /// The running slot keeps the CPU unless its slice has run out or a
    /// higher-priority slot is ready. If it loses the CPU it goes to the back
    /// of its level. The new slot gets a fresh slice from [`quantum`].
    pub fn reschedule(&mut self) -> Option<usize> {
        let expired = self.running.is_some() && self.budget == 0;
        if self.running.is_some() && !expired && !self.higher_ready() {
            return self.running;
        }
        if let Some(prev) = self.running.take() {
            // The previous slot is ready again. The level has room for it
            // because each slot is in at most one place.
            let level = self.priority[prev] as usize;
            let placed = self.queues[level].push_back(prev);
            debug_assert!(placed.is_ok(), "a slot is always queued at most once");
            self.state[prev] = SlotState::Ready;
        }
        let level = self.best_ready_level()?;
        let next = self.queues[level].pop_front()?;
        self.state[next] = SlotState::Running;
        self.running = Some(next);
        self.budget = quantum(self.priority[next]);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Sched = PrioritySched<8>;

    fn sched_with(threads: &[(usize, u8)]) -> Sched {
        let mut s = Sched::new();
        for &(slot, prio) in threads {
            s.add(slot, prio).unwrap();
        }
        s
    }

    #[test]
    fn highest_priority_is_always_picked() {
        let mut s = sched_with(&[(0, 20), (1, 5), (2, 30)]);
        assert_eq!(s.reschedule(), Some(1));
        assert_eq!(s.running(), Some(1));
        // Still the best while it runs; no preemption without a tick or wake.
        assert_eq!(s.reschedule(), Some(1));
    }

    #[test]
    fn round_robin_within_a_level_preserves_order() {
        let mut s = sched_with(&[(0, 10), (1, 10), (2, 10)]);
        let mut order = [0usize; 6];
        for slot in order.iter_mut() {
            // Burn the slice.
            while !s.on_tick() {}
            *slot = s.reschedule().unwrap();
        }
        // 0 runs first, then 1, 2, and the cycle repeats.
        assert_eq!(order, [0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn tick_that_exhausts_the_quantum_requests_preemption() {
        let mut s = sched_with(&[(0, 0), (1, 0)]);
        s.reschedule();
        // Level 0 has a quantum of 2 ticks.
        assert!(!s.on_tick());
        assert!(s.on_tick());
        assert_eq!(s.reschedule(), Some(1));
    }

    #[test]
    fn lone_thread_keeps_running_after_its_slice() {
        let mut s = sched_with(&[(0, 16)]);
        s.reschedule();
        for _ in 0..quantum(16) {
            s.on_tick();
        }
        assert_eq!(s.reschedule(), Some(0));
        assert_eq!(s.state(0), Some(SlotState::Running));
    }

    #[test]
    fn wakeup_of_higher_priority_preempts_on_next_decision() {
        // Slot 0 is the high-priority thread; slot 1 runs while it is blocked.
        let mut s = sched_with(&[(0, 3), (1, 20)]);
        assert_eq!(s.reschedule(), Some(0));
        s.block(0).unwrap();
        assert_eq!(s.reschedule(), Some(1));
        s.unblock(0).unwrap();
        // The woken thread outranks the runner, so a tick asks for a switch.
        assert!(s.on_tick());
        assert_eq!(s.reschedule(), Some(0));
        // The preempted thread went to the back of its level, still ready.
        assert_eq!(s.state(1), Some(SlotState::Ready));
    }

    #[test]
    fn blocked_threads_are_never_picked() {
        let mut s = sched_with(&[(0, 4), (1, 4)]);
        s.block(0).unwrap();
        assert_eq!(s.reschedule(), Some(1));
        s.block_running();
        assert_eq!(s.reschedule(), None);
        assert_eq!(s.state(0), Some(SlotState::Blocked));
        s.unblock(0).unwrap();
        assert_eq!(s.reschedule(), Some(0));
    }

    #[test]
    fn full_fifo_refuses_push_without_overwriting() {
        let mut f = Fifo::<2>::new();
        f.push_back(4).unwrap();
        f.push_back(5).unwrap();
        assert_eq!(f.push_back(6), Err(SchedError::Full));
        assert_eq!(f.pop_front(), Some(4));
        assert_eq!(f.pop_front(), Some(5));
        assert_eq!(f.pop_front(), None);
    }

    #[test]
    fn add_refuses_bad_and_duplicate_arguments() {
        let mut s = sched_with(&[(0, 7)]);
        assert_eq!(s.add(0, 7), Err(SchedError::AlreadyRegistered));
        assert_eq!(s.add(9, 7), Err(SchedError::BadSlot));
        assert_eq!(s.add(1, 40), Err(SchedError::BadPriority));
        // A refused add leaves the table as it was.
        assert_eq!(s.state(1), Some(SlotState::Free));
    }

    #[test]
    fn unblock_refuses_a_slot_that_is_not_blocked() {
        let mut s = sched_with(&[(0, 4)]);
        assert_eq!(s.unblock(0), Err(SchedError::NotBlocked));
        assert_eq!(s.unblock(3), Err(SchedError::Unknown));
    }

    #[test]
    fn exit_removes_a_ready_slot_from_its_queue() {
        let mut s = sched_with(&[(0, 9), (1, 9)]);
        s.exit(0).unwrap();
        assert_eq!(s.reschedule(), Some(1));
        assert_eq!(s.exit(0), Err(SchedError::Unknown));
    }

    #[test]
    fn quantum_shrinks_with_priority() {
        assert_eq!(quantum(0), 2);
        assert_eq!(quantum(8), 4);
        assert_eq!(quantum(16), 8);
        assert_eq!(quantum(31), 16);
        assert!(quantum(0) <= quantum(31));
    }
}
