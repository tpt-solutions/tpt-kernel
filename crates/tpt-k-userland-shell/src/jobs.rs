// SPDX-License-Identifier: MIT OR Apache-2.0
//! Job control: a table of jobs with ids, states and the `fg`/`bg` transitions.
//!
//! The table holds bookkeeping only. Starting and stopping processes belongs to
//! the process layer, which calls [`JobTable::stop`] and [`JobTable::finish`]
//! when it sees a change. Job specs follow the usual shell forms: `%n` or `n`
//! for job number n, `%%` or `%+` for the current job, and `%-` for the
//! previous one.

use std::string::String;
use std::vec::Vec;

/// What a job is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    /// Running, in the foreground or the background.
    Running,
    /// Stopped by a signal and waiting to be continued.
    Stopped,
    /// Finished, with its exit code.
    Done(i32),
}

/// One job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// Job number, counted from 1 and not reused while the table lives.
    pub id: u32,
    /// The command line, as typed.
    pub command: String,
    /// Current state.
    pub state: JobState,
    /// True when the job runs in the background.
    pub background: bool,
}

/// Why a job operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobError {
    /// No job has that id, or there is no current or previous job.
    NoSuchJob,
    /// The spec text is not a job spec.
    BadSpec,
    /// The job has finished, so it cannot be continued.
    AlreadyDone,
}

/// The job table.
#[derive(Clone, Debug, Default)]
pub struct JobTable {
    jobs: Vec<Job>,
    next_id: u32,
}

impl JobTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            next_id: 1,
        }
    }

    /// Adds a running job and returns its id.
    pub fn add(&mut self, command: &str, background: bool) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job {
            id,
            command: String::from(command),
            state: JobState::Running,
            background,
        });
        id
    }

    /// The job with `id`, if the table still holds it.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// All jobs, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter()
    }

    fn get_mut(&mut self, id: u32) -> Result<&mut Job, JobError> {
        self.jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(JobError::NoSuchJob)
    }

    /// The current job: the most recently added job that is not done.
    #[must_use]
    pub fn current(&self) -> Option<u32> {
        self.jobs
            .iter()
            .rev()
            .find(|j| !matches!(j.state, JobState::Done(_)))
            .map(|j| j.id)
    }

    /// The previous job: the one before the current, among jobs not done.
    #[must_use]
    pub fn previous(&self) -> Option<u32> {
        self.jobs
            .iter()
            .rev()
            .filter(|j| !matches!(j.state, JobState::Done(_)))
            .nth(1)
            .map(|j| j.id)
    }

    /// Resolves a job spec to an id. Does not check that the job exists.
    pub fn resolve(&self, spec: &str) -> Result<u32, JobError> {
        match spec {
            "%%" | "%+" | "%" => self.current().ok_or(JobError::NoSuchJob),
            "%-" => self.previous().ok_or(JobError::NoSuchJob),
            _ => {
                let digits = spec.strip_prefix('%').unwrap_or(spec);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(JobError::BadSpec);
                }
                digits.parse::<u32>().map_err(|_| JobError::BadSpec)
            }
        }
    }

    /// Makes a job the foreground job: it is no longer background, and a
    /// stopped job runs again. Returns the job.
    pub fn foreground(&mut self, id: u32) -> Result<&Job, JobError> {
        let job = self.get_mut(id)?;
        if matches!(job.state, JobState::Done(_)) {
            return Err(JobError::AlreadyDone);
        }
        job.background = false;
        job.state = JobState::Running;
        Ok(job)
    }

    /// Continues a stopped job in the background. A job already running in the
    /// background is left as it is. Returns the job.
    pub fn background(&mut self, id: u32) -> Result<&Job, JobError> {
        let job = self.get_mut(id)?;
        match job.state {
            JobState::Done(_) => return Err(JobError::AlreadyDone),
            JobState::Stopped => job.state = JobState::Running,
            JobState::Running => {}
        }
        job.background = true;
        Ok(job)
    }

    /// Records that a running job stopped (for example on Ctrl-Z). A job that
    /// is already done stays done.
    pub fn stop(&mut self, id: u32) -> Result<(), JobError> {
        let job = self.get_mut(id)?;
        if job.state == JobState::Running {
            job.state = JobState::Stopped;
        }
        Ok(())
    }

    /// Records that a job finished with `code`. The first finish wins.
    pub fn finish(&mut self, id: u32, code: i32) -> Result<(), JobError> {
        let job = self.get_mut(id)?;
        if !matches!(job.state, JobState::Done(_)) {
            job.state = JobState::Done(code);
        }
        Ok(())
    }

    /// Removes every finished job and returns them, oldest first, so the shell
    /// can print their notices once.
    pub fn reap(&mut self) -> Vec<Job> {
        let mut done = Vec::new();
        let mut kept = Vec::with_capacity(self.jobs.len());
        for job in self.jobs.drain(..) {
            if matches!(job.state, JobState::Done(_)) {
                done.push(job);
            } else {
                kept.push(job);
            }
        }
        self.jobs = kept;
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_count_up_and_are_not_reused_after_reap() {
        let mut t = JobTable::new();
        let a = t.add("sleep 1", true);
        let b = t.add("make", false);
        assert_eq!((a, b), (1, 2));
        t.finish(a, 0).unwrap();
        assert_eq!(t.reap().len(), 1);
        assert_eq!(t.add("ls", false), 3);
    }

    #[test]
    fn stop_and_background_then_foreground() {
        let mut t = JobTable::new();
        let id = t.add("vi f", false);
        t.stop(id).unwrap();
        assert_eq!(t.get(id).unwrap().state, JobState::Stopped);
        // bg continues it in the background.
        let job = t.background(id).unwrap();
        assert_eq!((job.state, job.background), (JobState::Running, true));
        // fg brings it to the foreground.
        let job = t.foreground(id).unwrap();
        assert_eq!((job.state, job.background), (JobState::Running, false));
    }

    #[test]
    fn background_on_running_job_is_a_no_op_apart_from_the_flag() {
        let mut t = JobTable::new();
        let id = t.add("x", false);
        let job = t.background(id).unwrap();
        assert_eq!(job.state, JobState::Running);
        assert!(job.background);
    }

    #[test]
    fn stopping_a_done_job_does_not_revive_it() {
        let mut t = JobTable::new();
        let id = t.add("x", true);
        t.finish(id, 3).unwrap();
        t.stop(id).unwrap();
        assert_eq!(t.get(id).unwrap().state, JobState::Done(3));
        assert_eq!(t.foreground(id).unwrap_err(), JobError::AlreadyDone);
        assert_eq!(t.background(id).unwrap_err(), JobError::AlreadyDone);
    }

    #[test]
    fn first_finish_wins() {
        let mut t = JobTable::new();
        let id = t.add("x", true);
        t.finish(id, 0).unwrap();
        t.finish(id, 9).unwrap();
        assert_eq!(t.get(id).unwrap().state, JobState::Done(0));
    }

    #[test]
    fn unknown_job_is_reported() {
        let mut t = JobTable::new();
        assert_eq!(t.foreground(7).unwrap_err(), JobError::NoSuchJob);
        assert_eq!(t.stop(7).unwrap_err(), JobError::NoSuchJob);
        assert_eq!(t.finish(7, 0).unwrap_err(), JobError::NoSuchJob);
    }

    #[test]
    fn specs_resolve_to_ids() {
        let mut t = JobTable::new();
        let a = t.add("a", true);
        let b = t.add("b", true);
        assert_eq!(t.resolve("%%").unwrap(), b);
        assert_eq!(t.resolve("%+").unwrap(), b);
        assert_eq!(t.resolve("%-").unwrap(), a);
        assert_eq!(t.resolve("%1").unwrap(), a);
        assert_eq!(t.resolve("2").unwrap(), b);
        assert_eq!(t.resolve("%x"), Err(JobError::BadSpec));
        assert_eq!(t.resolve("%"), Ok(b));
        assert_eq!(t.resolve("%"), t.resolve("%%"));
        assert_eq!(t.resolve("%"), Ok(b));
        assert_eq!(t.resolve("-1"), Err(JobError::BadSpec));
    }

    #[test]
    fn current_and_previous_skip_done_jobs() {
        let mut t = JobTable::new();
        let a = t.add("a", true);
        let b = t.add("b", true);
        let c = t.add("c", true);
        t.finish(c, 0).unwrap();
        assert_eq!(t.current(), Some(b));
        assert_eq!(t.previous(), Some(a));
        t.finish(b, 0).unwrap();
        assert_eq!(t.current(), Some(a));
        assert_eq!(t.previous(), None);
        assert_eq!(t.resolve("%-"), Err(JobError::NoSuchJob));
    }

    #[test]
    fn reap_removes_only_done_jobs_in_order() {
        let mut t = JobTable::new();
        let a = t.add("a", true);
        let b = t.add("b", true);
        let c = t.add("c", true);
        t.finish(c, 1).unwrap();
        t.finish(a, 0).unwrap();
        let done = t.reap();
        assert_eq!(done.iter().map(|j| j.id).collect::<Vec<_>>(), [a, c]);
        assert_eq!(done[1].state, JobState::Done(1));
        assert_eq!(t.iter().map(|j| j.id).collect::<Vec<_>>(), [b]);
        assert!(t.reap().is_empty());
    }
}
