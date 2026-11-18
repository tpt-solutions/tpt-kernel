// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k kernel scheduler. Phase 2 has a cooperative round-robin scheduler over
//! a fixed thread table, and the x86_64 context switch. Preemption needs the
//! timer (APIC), which comes later, so a thread runs until it yields.
//!
//! The scheduler is single-core and is not re-entered. Callers must not run
//! [`Scheduler::yield_now`] from two places at once.

#![no_std]

pub mod context;
pub mod priority;

pub use context::{StackError, ENTRY_WORD, FRAME_WORDS};

/// Threads the table holds, including the bootstrap thread.
pub const MAX_THREADS: usize = 16;

/// Lifecycle of a thread slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The slot is unused.
    Free,
    /// The thread can run.
    Ready,
    /// The thread waits for an event (an IPC partner). It is not scheduled
    /// until [`Scheduler::wake`] makes it ready again.
    Blocked,
    /// The thread has exited and is never scheduled again.
    Done,
}

/// One entry in the thread table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    /// Saved stack pointer. Valid while the thread is not running.
    pub rsp: u64,
    /// Lifecycle state.
    pub state: State,
}

const FREE_THREAD: Thread = Thread {
    rsp: 0,
    state: State::Free,
};

/// Why a thread could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// Every slot is in use.
    TableFull,
    /// The stack cannot hold the initial frame.
    Stack(StackError),
}

/// Round-robin scheduler over [`MAX_THREADS`] slots. Slot 0 is the bootstrap
/// thread, which is running when the scheduler is created.
#[derive(Debug)]
pub struct Scheduler {
    threads: [Thread; MAX_THREADS],
    current: usize,
    switch_hook: Option<fn(usize)>,
}

impl Scheduler {
    /// A scheduler with the bootstrap thread as the only, running, thread.
    #[must_use]
    pub const fn new() -> Self {
        let mut threads = [FREE_THREAD; MAX_THREADS];
        threads[0].state = State::Ready;
        Self {
            threads,
            current: 0,
            switch_hook: None,
        }
    }

    /// Registers a function that runs before every switch, with the slot of the
    /// incoming thread. The kernel uses it to install that thread's kernel stack
    /// and address space.
    pub fn set_switch_hook(&mut self, hook: fn(usize)) {
        self.switch_hook = Some(hook);
    }

    /// Index of the running thread.
    #[must_use]
    pub fn current(&self) -> usize {
        self.current
    }

    /// The thread table.
    #[must_use]
    pub fn threads(&self) -> &[Thread; MAX_THREADS] {
        &self.threads
    }

    /// Creates a thread that starts at `entry` on `stack`, and returns its slot.
    ///
    /// `stack` is the thread's whole stack, and its end must be 16-byte aligned.
    /// The thread is ready to run but has not started.
    ///
    /// # Errors
    ///
    /// Returns [`SpawnError::TableFull`] when no slot is free, and
    /// [`SpawnError::Stack`] when the stack cannot hold the initial frame.
    pub fn spawn(&mut self, stack: &mut [u64], entry: u64) -> Result<usize, SpawnError> {
        let slot = self
            .threads
            .iter()
            .position(|t| t.state == State::Free)
            .ok_or(SpawnError::TableFull)?;
        let rsp = context::initial_stack(stack, entry).map_err(SpawnError::Stack)?;
        self.threads[slot] = Thread {
            rsp,
            state: State::Ready,
        };
        Ok(slot)
    }

    /// The next ready thread after `after`, searching round-robin. `after`
    /// itself is considered last, so a lone ready thread is returned to itself.
    #[must_use]
    pub fn pick_next(&self, after: usize) -> Option<usize> {
        (1..=MAX_THREADS)
            .map(|step| (after + step) % MAX_THREADS)
            .find(|&index| self.threads[index].state == State::Ready)
    }

    /// Gives the processor to the next ready thread, if there is another one.
    /// Returns when this thread is scheduled again, with `true` if the processor
    /// was given away and `false` if no other thread was ready.
    ///
    /// # Safety
    ///
    /// Must be called from the running thread, with `table` pointing at the table
    /// that all threads share. The stacks of the threads must be live. No reference
    /// into the table may be live across this call: the other threads use the table
    /// while this one is switched out, so the switch works through raw pointers.
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn yield_now(table: *mut Self) -> bool {
        let (save, next_rsp, hook, next) = {
            // SAFETY: `table` is the shared table, used only by the running thread
            // here. This reference ends before the switch.
            let s = unsafe { &mut *table };
            let Some(next) = s.pick_next(s.current) else {
                return false;
            };
            let cur = s.current;
            if next == cur {
                return false;
            }
            s.current = next;
            let next_rsp = s.threads[next].rsp;
            let hook = s.switch_hook;
            // The save slot is named by a raw pointer, so no reference outlives
            // the switch.
            (
                core::ptr::addr_of_mut!((*table).threads[cur].rsp),
                next_rsp,
                hook,
                next,
            )
        };
        if let Some(hook) = hook {
            hook(next);
        }
        // SAFETY: `save` points into the table and outlives the switch. The
        // table is only touched by running threads, one at a time. `next_rsp` was
        // written by `spawn` or by an earlier switch, and its stack is live (see
        // the function contract).
        unsafe { context::switch(save, next_rsp) };
        true
    }

    /// Makes thread `slot` ready again if it is blocked. Waking a thread that
    /// is not blocked does nothing.
    pub fn wake(&mut self, slot: usize) {
        if let Some(thread) = self.threads.get_mut(slot) {
            if thread.state == State::Blocked {
                thread.state = State::Ready;
            }
        }
    }

    /// Whether the running thread could block now: another thread is Ready to run
    /// in its place. A call that would block checks this first, so that it can be
    /// refused before it changes any state.
    #[must_use]
    pub fn can_block(&self) -> bool {
        self.pick_next(self.current).is_some()
    }

    /// Blocks the running thread and gives up the processor. Returns when the
    /// thread is woken and scheduled again.
    ///
    /// Returns `false` without blocking if no other thread is ready. Blocking
    /// then would deadlock, so the caller must decide what to do.
    ///
    /// # Safety
    ///
    /// Same contract as [`Scheduler::yield_now`].
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn block_current(table: *mut Self) -> bool {
        {
            // SAFETY: as for `yield_now`. This reference ends before the switch.
            let s = unsafe { &mut *table };
            let cur = s.current;
            s.threads[cur].state = State::Blocked;
            if s.pick_next(cur).is_none() {
                s.threads[cur].state = State::Ready;
                return false;
            }
        }
        // SAFETY: the caller upholds the `yield_now` contract. Another Ready thread
        // exists, so `yield_now` switches away and does not return early. The
        // running thread is marked Blocked, so it is not picked.
        unsafe { Self::yield_now(table) };
        true
    }

    /// Marks the running thread done and gives up the processor. Done threads
    /// are not scheduled again.
    ///
    /// # Safety
    ///
    /// Same contract as [`Scheduler::yield_now`]. The caller must not depend on
    /// anything after this call.
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn exit_current(table: *mut Self) {
        {
            // SAFETY: as for `yield_now`. This reference ends before the switch.
            let s = unsafe { &mut *table };
            let cur = s.current;
            s.threads[cur].state = State::Done;
        }
        // SAFETY: the caller upholds the `yield_now` contract. The thread is marked
        // Done, so it is never picked again. If no other thread is Ready,
        // `yield_now` returns at once and this thread keeps running. The caller
        // must then stop the processor itself.
        unsafe { Self::yield_now(table) };
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stack with a 16-byte aligned end, for tests.
    #[repr(C, align(16))]
    struct Stack([u64; 32]);

    #[test]
    fn bootstrap_thread_is_running_and_ready() {
        let sched = Scheduler::new();
        assert_eq!(sched.current(), 0);
        assert_eq!(sched.threads()[0].state, State::Ready);
        assert_eq!(sched.threads()[1].state, State::Free);
    }

    #[test]
    fn spawn_builds_the_initial_frame_at_the_top_of_the_stack() {
        let mut stack = Stack([0; 32]);
        let base = stack.0.as_ptr() as u64;
        let mut sched = Scheduler::new();
        let slot = sched.spawn(&mut stack.0, 0xDEAD_0000).unwrap();
        assert_eq!(slot, 1);
        let thread = sched.threads()[1];
        assert_eq!(thread.state, State::Ready);
        // The frame is the last FRAME_WORDS words, and rsp points at its start.
        let top = base + 32 * 8;
        assert_eq!(thread.rsp, top - (FRAME_WORDS as u64) * 8);
        assert_eq!(stack.0[32 - FRAME_WORDS + ENTRY_WORD], 0xDEAD_0000);
        assert!(stack.0[32 - FRAME_WORDS..32 - FRAME_WORDS + ENTRY_WORD]
            .iter()
            .all(|&w| w == 0));
        // After `ret` pops the entry, rsp is top - 8, which is 8 mod 16.
        assert_eq!((thread.rsp + 56) % 16, 8);
    }

    #[test]
    fn spawn_refuses_full_table_and_bad_stacks() {
        let mut sched = Scheduler::new();
        let mut stacks: [Stack; MAX_THREADS] = core::array::from_fn(|_| Stack([0; 32]));
        for stack in stacks.iter_mut().skip(1) {
            sched.spawn(&mut stack.0, 1).unwrap();
        }
        let mut extra = Stack([0; 32]);
        assert_eq!(sched.spawn(&mut extra.0, 1), Err(SpawnError::TableFull));

        let mut small = [0u64; 4];
        let mut fresh = Scheduler::new();
        assert_eq!(
            fresh.spawn(&mut small, 1),
            Err(SpawnError::Stack(StackError::TooSmall))
        );
        let mut odd = Stack([0; 32]);
        assert_eq!(
            fresh.spawn(&mut odd.0[..31], 1),
            Err(SpawnError::Stack(StackError::Misaligned))
        );
    }

    #[test]
    fn round_robin_order_and_skipping() {
        let mut sched = Scheduler::new();
        let mut a = Stack([0; 32]);
        let mut b = Stack([0; 32]);
        let mut c = Stack([0; 32]);
        sched.spawn(&mut a.0, 1).unwrap();
        sched.spawn(&mut b.0, 1).unwrap();
        sched.spawn(&mut c.0, 1).unwrap();
        assert_eq!(sched.pick_next(0), Some(1));
        assert_eq!(sched.pick_next(1), Some(2));
        assert_eq!(sched.pick_next(2), Some(3));
        assert_eq!(sched.pick_next(3), Some(0));
        // A done thread is skipped.
        sched.threads[2].state = State::Done;
        assert_eq!(sched.pick_next(1), Some(3));
    }

    #[test]
    fn blocked_threads_are_skipped_until_woken() {
        let mut sched = Scheduler::new();
        let mut a = Stack([0; 32]);
        sched.spawn(&mut a.0, 1).unwrap();
        sched.threads[1].state = State::Blocked;
        assert_eq!(sched.pick_next(0), Some(0));
        sched.wake(1);
        assert_eq!(sched.threads()[1].state, State::Ready);
        assert_eq!(sched.pick_next(0), Some(1));
        // Waking a thread that is not blocked changes nothing.
        sched.threads[0].state = State::Done;
        sched.wake(0);
        assert_eq!(sched.threads()[0].state, State::Done);
    }

    #[test]
    fn a_lone_ready_thread_picks_itself_and_nothing_is_ready_when_all_are_done() {
        let mut sched = Scheduler::new();
        assert_eq!(sched.pick_next(0), Some(0));
        sched.threads[0].state = State::Done;
        assert_eq!(sched.pick_next(0), None);
    }
}
