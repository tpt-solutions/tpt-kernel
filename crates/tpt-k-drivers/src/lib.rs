// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k driver manager: the restart policy for user-space drivers.
//!
//! The manager is a deterministic state machine. It does not spawn processes
//! or read clocks. The caller reports what happened (a start, a heartbeat, an
//! exit) with the current tick, asks for time to pass with [`Manager::tick`],
//! and carries out the [`Action`]s it returns: restart a driver, or kill one
//! whose heartbeat has stopped.
//!
//! A driver that exits without a stop request is a crash. Each consecutive
//! crash waits longer before the restart (exponential backoff up to a cap).
//! After more than `max_restarts` consecutive crashes the driver is given up
//! on. A driver that runs for `reset_after` ticks before it crashes starts a
//! new streak.

#![no_std]

#[cfg(test)]
extern crate std;

/// Time in caller-defined ticks. The manager only compares and subtracts them.
pub type Tick = u64;

/// Identifies a driver in a [`Manager`]. Ids are never reused.
pub type DriverId = u32;

/// Restart and health parameters for one driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Restarts allowed in one streak of crashes. The crash after that gives
    /// the driver up.
    pub max_restarts: u32,
    /// A running driver whose last heartbeat is more than this many ticks old
    /// is killed.
    pub heartbeat_timeout: Tick,
    /// The delay before the first restart of a streak. Each further crash in
    /// the streak doubles it.
    pub backoff_base: Tick,
    /// The longest delay the doubling reaches.
    pub backoff_cap: Tick,
    /// A run at least this long before a crash ends the streak, so the next
    /// crash starts a new one.
    pub reset_after: Tick,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            heartbeat_timeout: 300,
            backoff_base: 10,
            backoff_cap: 640,
            reset_after: 1000,
        }
    }
}

/// Where a driver is in its life cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverState {
    /// Spawned or restarted, not yet reporting. The heartbeat clock runs.
    Starting,
    /// Reporting heartbeats.
    Running,
    /// Killed for a missed heartbeat. Waiting for its exit to be reported.
    Failed,
    /// Crashed. Restarts when the tick reaches `until`.
    Backoff {
        /// Tick at which the restart is due.
        until: Tick,
    },
    /// Stopped on request, or after a clean exit with a stop request.
    Stopped,
    /// Too many consecutive crashes. The driver is not restarted again.
    GaveUp,
}

/// Something the caller must do, or should record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Spawn the driver again.
    Restart {
        /// The driver to restart.
        id: DriverId,
    },
    /// Kill the driver process. Its exit is reported afterwards with
    /// [`Manager::exited`].
    Kill {
        /// The driver to kill.
        id: DriverId,
    },
    /// A message for the log.
    Log(LogEvent),
}

/// A log message produced by the manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogEvent {
    /// A driver crashed. `code` is `None` when no exit status was reported.
    Crashed {
        /// The driver that crashed.
        id: DriverId,
        /// Exit status, if one was reported.
        code: Option<i32>,
        /// Crashes in the current streak, including this one.
        consecutive: u32,
    },
    /// A driver was given up on after too many crashes.
    GaveUp {
        /// The driver that was given up on.
        id: DriverId,
        /// Crashes in the final streak.
        crashes: u32,
    },
    /// A driver exited with a non-zero status after it was asked to stop.
    StoppedWithError {
        /// The driver that stopped.
        id: DriverId,
        /// The exit status.
        code: i32,
    },
}

/// One supervised driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Driver {
    /// The driver's id in its manager.
    pub id: DriverId,
    /// A name for logs.
    pub name: &'static str,
    /// Current state.
    pub state: DriverState,
    /// The restart policy.
    pub policy: Policy,
    /// Restarts since the driver was added.
    pub restarts: u32,
    /// Crashes in the current streak.
    pub consecutive_crashes: u32,
    /// Tick of the last heartbeat, or of the last start or restart.
    pub last_heartbeat: Tick,
    run_since: Tick,
    stop_requested: bool,
}

impl Driver {
    fn new(id: DriverId, name: &'static str, policy: Policy, now: Tick) -> Self {
        Self {
            id,
            name,
            state: DriverState::Starting,
            policy,
            restarts: 0,
            consecutive_crashes: 0,
            last_heartbeat: now,
            run_since: now,
            stop_requested: false,
        }
    }

    /// The driver reports it is up.
    fn started(&mut self, now: Tick) {
        if self.state == DriverState::Starting {
            self.state = DriverState::Running;
            self.last_heartbeat = now;
            self.run_since = now;
        }
    }

    /// The driver reports it is alive.
    fn heartbeat(&mut self, now: Tick) {
        if matches!(self.state, DriverState::Starting | DriverState::Running) {
            self.state = DriverState::Running;
            self.last_heartbeat = now;
        }
    }

    /// The driver's process exited with `code`.
    fn exited(&mut self, now: Tick, code: i32) -> Option<Action> {
        if !matches!(
            self.state,
            DriverState::Starting | DriverState::Running | DriverState::Failed
        ) {
            return None;
        }
        if self.stop_requested {
            self.state = DriverState::Stopped;
            return (code != 0).then_some(Action::Log(LogEvent::StoppedWithError {
                id: self.id,
                code,
            }));
        }
        self.crash(now, Some(code))
    }

    /// Records a crash and chooses between backoff and giving up.
    fn crash(&mut self, now: Tick, code: Option<i32>) -> Option<Action> {
        if now.saturating_sub(self.run_since) >= self.policy.reset_after {
            self.consecutive_crashes = 0;
        }
        self.consecutive_crashes = self.consecutive_crashes.saturating_add(1);
        if self.consecutive_crashes > self.policy.max_restarts {
            self.state = DriverState::GaveUp;
            return Some(Action::Log(LogEvent::GaveUp {
                id: self.id,
                crashes: self.consecutive_crashes,
            }));
        }
        let delay = backoff_delay(&self.policy, self.consecutive_crashes);
        self.state = DriverState::Backoff {
            until: now.saturating_add(delay),
        };
        Some(Action::Log(LogEvent::Crashed {
            id: self.id,
            code,
            consecutive: self.consecutive_crashes,
        }))
    }

    /// Lets time pass. Kills a driver whose heartbeat is overdue, restarts a
    /// driver whose backoff has ended, and treats a killed driver that never
    /// reported its exit as crashed after twice the heartbeat timeout.
    fn tick(&mut self, now: Tick) -> Option<Action> {
        match self.state {
            DriverState::Starting | DriverState::Running => {
                if now.saturating_sub(self.last_heartbeat) > self.policy.heartbeat_timeout {
                    self.state = DriverState::Failed;
                    Some(Action::Kill { id: self.id })
                } else {
                    None
                }
            }
            DriverState::Failed => {
                let limit = self.policy.heartbeat_timeout.saturating_mul(2);
                if now.saturating_sub(self.last_heartbeat) > limit {
                    self.crash(now, None)
                } else {
                    None
                }
            }
            DriverState::Backoff { until } if now >= until => {
                self.state = DriverState::Starting;
                self.restarts = self.restarts.saturating_add(1);
                self.last_heartbeat = now;
                self.run_since = now;
                Some(Action::Restart { id: self.id })
            }
            _ => None,
        }
    }

    /// Asks the driver to stop. A driver in backoff stops at once and is not
    /// restarted. A running one stops when its exit is reported.
    fn request_stop(&mut self) {
        self.stop_requested = true;
        if matches!(self.state, DriverState::Backoff { .. }) {
            self.state = DriverState::Stopped;
        }
    }
}

/// The delay before the restart that follows `consecutive` crashes in a
/// streak: `backoff_base` doubled `consecutive - 1` times, at most
/// `backoff_cap`.
#[must_use]
pub fn backoff_delay(policy: &Policy, consecutive: u32) -> Tick {
    let cap = policy.backoff_cap;
    let mut delay = policy.backoff_base;
    let mut step = 1;
    while step < consecutive && delay < cap && delay != 0 {
        delay = delay.saturating_mul(2);
        step += 1;
    }
    delay.min(cap)
}

/// A fixed-capacity set of supervised drivers. Holds at most `N` drivers.
#[derive(Debug)]
pub struct Manager<const N: usize> {
    slots: [Option<Driver>; N],
    next_id: DriverId,
}

impl<const N: usize> Default for Manager<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Manager<N> {
    /// An empty manager.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; N],
            next_id: 1,
        }
    }

    /// Adds a driver in the `Starting` state and returns its id, or `None` if
    /// the manager is full. The caller spawns the process.
    pub fn add(&mut self, name: &'static str, policy: Policy, now: Tick) -> Option<DriverId> {
        let slot = self.slots.iter_mut().find(|s| s.is_none())?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1)?;
        *slot = Some(Driver::new(id, name, policy, now));
        Some(id)
    }

    /// The driver with `id`, if it is present.
    #[must_use]
    pub fn get(&self, id: DriverId) -> Option<&Driver> {
        self.slots.iter().flatten().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: DriverId) -> Option<&mut Driver> {
        self.slots.iter_mut().flatten().find(|d| d.id == id)
    }

    /// Reports that driver `id` started.
    pub fn started(&mut self, id: DriverId, now: Tick) {
        if let Some(d) = self.get_mut(id) {
            d.started(now);
        }
    }

    /// Reports a heartbeat from driver `id`.
    pub fn heartbeat(&mut self, id: DriverId, now: Tick) {
        if let Some(d) = self.get_mut(id) {
            d.heartbeat(now);
        }
    }

    /// Reports that driver `id` exited with `code`. Returns a log message if
    /// there is one to record.
    pub fn exited(&mut self, id: DriverId, now: Tick, code: i32) -> Option<Action> {
        self.get_mut(id)?.exited(now, code)
    }

    /// Asks driver `id` to stop, and it will not be restarted.
    pub fn request_stop(&mut self, id: DriverId) {
        if let Some(d) = self.get_mut(id) {
            d.request_stop();
        }
    }

    /// Lets time pass for every driver and passes each resulting action to
    /// `emit`.
    pub fn tick(&mut self, now: Tick, mut emit: impl FnMut(Action)) {
        for d in self.slots.iter_mut().flatten() {
            if let Some(action) = d.tick(now) {
                emit(action);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const POLICY: Policy = Policy {
        max_restarts: 3,
        heartbeat_timeout: 100,
        backoff_base: 10,
        backoff_cap: 80,
        reset_after: 1000,
    };

    fn one(policy: Policy) -> (Manager<4>, DriverId) {
        let mut m = Manager::new();
        let id = m.add("net", policy, 0).unwrap();
        m.started(id, 0);
        (m, id)
    }

    fn state<const N: usize>(m: &Manager<N>, id: DriverId) -> DriverState {
        m.get(id).unwrap().state
    }

    fn ticks(m: &mut Manager<4>, now: Tick) -> Vec<Action> {
        let mut out = Vec::new();
        m.tick(now, |a| out.push(a));
        out
    }

    #[test]
    fn crash_restarts_after_the_first_backoff() {
        let (mut m, id) = one(POLICY);
        let log = m.exited(id, 100, 1);
        assert_eq!(
            log,
            Some(Action::Log(LogEvent::Crashed {
                id,
                code: Some(1),
                consecutive: 1
            }))
        );
        assert_eq!(state(&m, id), DriverState::Backoff { until: 110 });
        assert!(ticks(&mut m, 109).is_empty());
        assert_eq!(ticks(&mut m, 110), [Action::Restart { id }]);
        assert_eq!(state(&m, id), DriverState::Starting);
        assert_eq!(m.get(id).unwrap().restarts, 1);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_delay(&POLICY, 1), 10);
        assert_eq!(backoff_delay(&POLICY, 2), 20);
        assert_eq!(backoff_delay(&POLICY, 3), 40);
        assert_eq!(backoff_delay(&POLICY, 4), 80);
        assert_eq!(backoff_delay(&POLICY, 50), 80);
        assert_eq!(backoff_delay(&POLICY, u32::MAX), 80);
        let zero = Policy {
            backoff_base: 0,
            ..POLICY
        };
        assert_eq!(backoff_delay(&zero, 1000), 0);
    }

    #[test]
    fn each_crash_in_a_streak_waits_longer() {
        let mut policy = POLICY;
        policy.max_restarts = 10;
        let (mut m, id) = one(policy);
        let mut now = 0;
        let mut waits = Vec::new();
        for _ in 0..4 {
            m.exited(id, now, 1);
            let DriverState::Backoff { until } = state(&m, id) else {
                panic!("expected backoff");
            };
            waits.push(until - now);
            now = until;
            assert_eq!(ticks(&mut m, now), [Action::Restart { id }]);
            m.started(id, now);
        }
        assert_eq!(waits, [10, 20, 40, 80]);
    }

    #[test]
    fn gives_up_after_too_many_consecutive_crashes() {
        let (mut m, id) = one(POLICY);
        let mut now = 0;
        for _ in 0..3 {
            m.exited(id, now, 1);
            let DriverState::Backoff { until } = state(&m, id) else {
                panic!("expected backoff");
            };
            now = until;
            ticks(&mut m, now);
            m.started(id, now);
        }
        // The fourth crash in the streak exceeds max_restarts = 3.
        let log = m.exited(id, now, 1);
        assert_eq!(log, Some(Action::Log(LogEvent::GaveUp { id, crashes: 4 })));
        assert_eq!(state(&m, id), DriverState::GaveUp);
        assert!(ticks(&mut m, now + 1_000_000).is_empty());
        assert_eq!(state(&m, id), DriverState::GaveUp);
    }

    #[test]
    fn a_stable_run_resets_the_crash_streak() {
        let (mut m, id) = one(POLICY);
        m.exited(id, 0, 1);
        ticks(&mut m, 10);
        m.started(id, 10);
        assert_eq!(m.get(id).unwrap().consecutive_crashes, 1);
        // Run for reset_after ticks, heartbeating, then crash.
        m.heartbeat(id, 500);
        m.heartbeat(id, 1009);
        let log = m.exited(id, 1010, 1);
        assert_eq!(
            log,
            Some(Action::Log(LogEvent::Crashed {
                id,
                code: Some(1),
                consecutive: 1
            }))
        );
        // The new streak starts at the base delay, not the doubled one.
        assert_eq!(state(&m, id), DriverState::Backoff { until: 1020 });
    }

    #[test]
    fn hung_driver_is_killed_then_restarted() {
        let (mut m, id) = one(POLICY);
        m.heartbeat(id, 50);
        // Overdue means strictly more than heartbeat_timeout since the last beat.
        assert!(ticks(&mut m, 150).is_empty());
        assert_eq!(ticks(&mut m, 151), [Action::Kill { id }]);
        assert_eq!(state(&m, id), DriverState::Failed);
        // The kill's exit is reported as a crash, then the restart follows.
        m.exited(id, 160, -9);
        assert_eq!(state(&m, id), DriverState::Backoff { until: 170 });
        assert_eq!(ticks(&mut m, 170), [Action::Restart { id }]);
    }

    #[test]
    fn a_killed_driver_that_never_exits_is_crashed_eventually() {
        let (mut m, id) = one(POLICY);
        assert_eq!(ticks(&mut m, 101), [Action::Kill { id }]);
        // Within twice the timeout, nothing more happens.
        assert!(ticks(&mut m, 200).is_empty());
        let out = ticks(&mut m, 201);
        assert_eq!(
            out,
            [Action::Log(LogEvent::Crashed {
                id,
                code: None,
                consecutive: 1
            })]
        );
        assert!(matches!(state(&m, id), DriverState::Backoff { .. }));
    }

    #[test]
    fn stop_request_prevents_restart() {
        let (mut m, id) = one(POLICY);
        m.request_stop(id);
        assert_eq!(
            m.exited(id, 5, 1),
            Some(Action::Log(LogEvent::StoppedWithError { id, code: 1 }))
        );
        assert_eq!(state(&m, id), DriverState::Stopped);
        assert!(ticks(&mut m, 1_000).is_empty());
    }

    #[test]
    fn stop_during_backoff_cancels_the_restart() {
        let (mut m, id) = one(POLICY);
        m.exited(id, 0, 1);
        assert!(matches!(state(&m, id), DriverState::Backoff { .. }));
        m.request_stop(id);
        assert_eq!(state(&m, id), DriverState::Stopped);
        assert!(ticks(&mut m, 1_000).is_empty());
    }

    #[test]
    fn clean_exit_after_stop_is_not_a_crash() {
        let (mut m, id) = one(POLICY);
        m.request_stop(id);
        assert_eq!(m.exited(id, 5, 0), None);
        assert_eq!(state(&m, id), DriverState::Stopped);
        assert_eq!(m.get(id).unwrap().consecutive_crashes, 0);
    }

    #[test]
    fn unexpected_clean_exit_is_a_crash() {
        let (mut m, id) = one(POLICY);
        let log = m.exited(id, 5, 0);
        assert_eq!(
            log,
            Some(Action::Log(LogEvent::Crashed {
                id,
                code: Some(0),
                consecutive: 1
            }))
        );
        assert!(matches!(state(&m, id), DriverState::Backoff { .. }));
    }

    #[test]
    fn events_for_unknown_or_finished_drivers_are_ignored() {
        let mut m: Manager<2> = Manager::new();
        assert_eq!(m.exited(42, 0, 1), None);
        m.heartbeat(42, 0);
        let id = m.add("a", POLICY, 0).unwrap();
        m.request_stop(id);
        // Stopped drivers stay stopped whatever is reported.
        m.exited(id, 1, 1);
        m.started(id, 2);
        assert_eq!(state(&m, id), DriverState::Stopped);
    }

    #[test]
    fn registry_is_fixed_capacity_and_ids_are_not_reused() {
        let mut m: Manager<2> = Manager::new();
        let a = m.add("a", POLICY, 0).unwrap();
        let b = m.add("b", POLICY, 0).unwrap();
        assert_ne!(a, b);
        assert_eq!(m.add("c", POLICY, 0), None);
        assert_eq!(m.get(b).unwrap().name, "b");
        assert!(m.get(99).is_none());
    }

    #[test]
    fn tick_reports_each_driver_separately() {
        let mut m: Manager<4> = Manager::new();
        let a = m.add("a", POLICY, 0).unwrap();
        let b = m.add("b", POLICY, 0).unwrap();
        m.started(a, 0);
        m.started(b, 0);
        m.heartbeat(b, 90);
        let mut out = Vec::new();
        m.tick(101, |x| out.push(x));
        // a missed its heartbeat; b beat at 90 and is still in time.
        assert_eq!(out, [Action::Kill { id: a }]);
    }
}
