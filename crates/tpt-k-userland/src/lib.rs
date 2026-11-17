// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k shell REPL: parses a line with `tpt-k-userland-shell`, runs the
//! builtins, and keeps the job table.
//!
//! Builtins: `jobs`, `fg`, `bg`, `exit`, `true`, `false` and `echo`. Any other
//! command reports `not supported yet` and exits with status 127, since this
//! tree cannot start programs yet. Redirections and pipelines are refused with
//! an error, and `&` records a job that is never run.
//!
//! Output goes through a [`Sink`], so tests capture it without a terminal.

#![forbid(unsafe_code)]

use std::io::BufRead;
use std::string::String;
use std::vec::Vec;

use tpt_k_userland_shell::{
    parse, Chain, Command, JobError, JobState, JobTable, ListItem, Pipeline, Redirect,
};

/// Where the shell writes its output.
pub trait Sink {
    /// Text for standard output.
    fn out(&mut self, text: &str);
    /// Text for standard error.
    fn err(&mut self, text: &str);
}

/// A [`Sink`] that keeps everything in memory. Useful for tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buffers {
    /// Everything written to standard output.
    pub stdout: String,
    /// Everything written to standard error.
    pub stderr: String,
}

impl Sink for Buffers {
    fn out(&mut self, text: &str) {
        self.stdout.push_str(text);
    }

    fn err(&mut self, text: &str) {
        self.stderr.push_str(text);
    }
}

/// What the REPL does after a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplOutcome {
    /// Keep reading. The value is the status of the last command.
    Continue(i32),
    /// Stop with this exit code.
    Exit(i32),
}

/// Result of running one pipeline or builtin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Status(i32),
    Exit(i32),
}

/// The shell's state between lines: the job table and the last status.
#[derive(Clone, Debug)]
pub struct Repl {
    jobs: JobTable,
    last: i32,
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

impl Repl {
    /// A REPL with no jobs and status 0. Job ids start at 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            jobs: JobTable::new(),
            last: 0,
        }
    }

    /// The job table.
    #[must_use]
    pub fn jobs(&self) -> &JobTable {
        &self.jobs
    }

    /// The status of the last command run.
    #[must_use]
    pub fn last_status(&self) -> i32 {
        self.last
    }

    /// Parses and runs one line. Stops early at `exit`.
    pub fn run_line(&mut self, line: &str, sink: &mut dyn Sink) -> ReplOutcome {
        let parsed = match parse(line) {
            Ok(p) => p,
            Err(e) => {
                sink.err(&format!("tpt-k: syntax error: {e:?}\n"));
                self.last = 2;
                return ReplOutcome::Continue(2);
            }
        };
        for item in &parsed.items {
            match self.run_item(item, sink) {
                Flow::Status(s) => self.last = s,
                Flow::Exit(code) => return ReplOutcome::Exit(code),
            }
        }
        ReplOutcome::Continue(self.last)
    }

    fn run_item(&mut self, item: &ListItem, sink: &mut dyn Sink) -> Flow {
        if item.background {
            let text = render_item(item);
            let id = self.jobs.add(&text, true);
            sink.out(&format!("[{id}]\n"));
            return Flow::Status(0);
        }
        let and_or = &item.and_or;
        let mut status = match self.run_pipeline(&and_or.first, sink) {
            Flow::Status(s) => s,
            exit => return exit,
        };
        for (chain, pipeline) in &and_or.rest {
            let run = match chain {
                Chain::And => status == 0,
                Chain::Or => status != 0,
            };
            if run {
                status = match self.run_pipeline(pipeline, sink) {
                    Flow::Status(s) => s,
                    exit => return exit,
                };
            }
        }
        Flow::Status(status)
    }

    fn run_pipeline(&mut self, pipeline: &Pipeline, sink: &mut dyn Sink) -> Flow {
        if pipeline.commands.len() > 1 {
            sink.err("tpt-k: pipelines are not supported yet\n");
            return Flow::Status(1);
        }
        let command = &pipeline.commands[0];
        if !command.redirects.is_empty() {
            sink.err("tpt-k: redirections are not supported yet\n");
            return Flow::Status(1);
        }
        self.builtin(command, sink)
    }

    fn builtin(&mut self, command: &Command, sink: &mut dyn Sink) -> Flow {
        let Some(name) = command.words.first() else {
            return Flow::Status(0);
        };
        let args = &command.words[1..];
        match name.as_str() {
            "true" => Flow::Status(0),
            "false" => Flow::Status(1),
            "echo" => {
                let mut text = args.join(" ");
                text.push('\n');
                sink.out(&text);
                Flow::Status(0)
            }
            "jobs" => {
                for job in self.jobs.iter() {
                    let state = match job.state {
                        JobState::Running => String::from("Running"),
                        JobState::Stopped => String::from("Stopped"),
                        JobState::Done(code) => format!("Done({code})"),
                    };
                    sink.out(&format!("[{}] {:<12}{}\n", job.id, state, job.command));
                }
                Flow::Status(0)
            }
            "fg" => self.continue_job(args, "fg", true, sink),
            "bg" => self.continue_job(args, "bg", false, sink),
            "exit" => self.exit(args, sink),
            _ => {
                sink.err(&format!("tpt-k: {name}: not supported yet\n"));
                Flow::Status(127)
            }
        }
    }

    fn continue_job(
        &mut self,
        args: &[String],
        name: &str,
        foreground: bool,
        sink: &mut dyn Sink,
    ) -> Flow {
        if args.len() > 1 {
            sink.err(&format!("tpt-k: {name}: usage: {name} [%n]\n"));
            return Flow::Status(2);
        }
        let spec = args.first().map_or("%%", String::as_str);
        let id = match self.jobs.resolve(spec) {
            Ok(id) => id,
            Err(e) => {
                sink.err(&format!("tpt-k: {name}: {}\n", job_error_text(e)));
                return Flow::Status(1);
            }
        };
        let result = if foreground {
            self.jobs.foreground(id).map(|j| j.command.clone())
        } else {
            self.jobs.background(id).map(|j| j.command.clone())
        };
        match result {
            Ok(command) => {
                sink.out(&format!("{command}\n"));
                Flow::Status(0)
            }
            Err(e) => {
                sink.err(&format!("tpt-k: {name}: {}\n", job_error_text(e)));
                Flow::Status(1)
            }
        }
    }

    fn exit(&mut self, args: &[String], sink: &mut dyn Sink) -> Flow {
        match args {
            [] => Flow::Exit(self.last),
            [code] => match code.parse::<i32>() {
                Ok(n) => Flow::Exit(n),
                Err(_) => {
                    sink.err("tpt-k: exit: numeric argument required\n");
                    Flow::Exit(2)
                }
            },
            _ => {
                sink.err("tpt-k: exit: too many arguments\n");
                Flow::Status(1)
            }
        }
    }
}

fn job_error_text(e: JobError) -> &'static str {
    match e {
        JobError::NoSuchJob => "no such job",
        JobError::BadSpec => "bad job spec",
        JobError::AlreadyDone => "job has finished",
    }
}

/// The text of a list item, used as the job's command line.
fn render_item(item: &ListItem) -> String {
    let mut text = render_pipeline(&item.and_or.first);
    for (chain, pipeline) in &item.and_or.rest {
        text.push_str(match chain {
            Chain::And => " && ",
            Chain::Or => " || ",
        });
        text.push_str(&render_pipeline(pipeline));
    }
    text.push_str(" &");
    text
}

fn render_pipeline(pipeline: &Pipeline) -> String {
    pipeline
        .commands
        .iter()
        .map(render_command)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn render_command(command: &Command) -> String {
    let mut parts = command.words.clone();
    for r in &command.redirects {
        match r {
            Redirect::Input(f) => parts.push(format!("< {f}")),
            Redirect::Output(f) => parts.push(format!("> {f}")),
            Redirect::Append(f) => parts.push(format!(">> {f}")),
        }
    }
    parts.join(" ")
}

/// Runs every line from `input` through a fresh or existing [`Repl`].
///
/// When `prompt` is true, `> ` is written before each line read. Stops at
/// `exit` or end of input. Returns the exit code: the `exit` argument, or the
/// last command's status at end of input.
pub fn run_reader<R: BufRead>(
    repl: &mut Repl,
    input: R,
    sink: &mut dyn Sink,
    prompt: bool,
) -> std::io::Result<i32> {
    let mut lines = input.lines();
    loop {
        if prompt {
            sink.out("> ");
        }
        let Some(line) = lines.next() else {
            return Ok(repl.last_status());
        };
        let line = line?;
        match repl.run_line(&line, sink) {
            ReplOutcome::Continue(_) => {}
            ReplOutcome::Exit(code) => return Ok(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str) -> (ReplOutcome, Buffers) {
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        let out = repl.run_line(line, &mut sink);
        (out, sink)
    }

    #[test]
    fn echo_joins_arguments() {
        let (out, sink) = run("echo hello   world");
        assert_eq!(out, ReplOutcome::Continue(0));
        assert_eq!(sink.stdout, "hello world\n");
        assert!(sink.stderr.is_empty());
    }

    #[test]
    fn true_and_false_set_status() {
        assert_eq!(run("true").0, ReplOutcome::Continue(0));
        assert_eq!(run("false").0, ReplOutcome::Continue(1));
    }

    #[test]
    fn and_or_short_circuit_follows_status() {
        let (_, sink) = run("false && echo skipped || echo ran");
        assert_eq!(sink.stdout, "ran\n");
        let (_, sink) = run("true && echo yes || echo no");
        assert_eq!(sink.stdout, "yes\n");
        let (_, sink) = run("true || echo never && echo after");
        assert_eq!(sink.stdout, "after\n");
    }

    #[test]
    fn semicolon_runs_regardless_of_status() {
        let (_, sink) = run("false; echo still");
        assert_eq!(sink.stdout, "still\n");
    }

    #[test]
    fn exit_stops_the_line_and_carries_its_code() {
        let (out, sink) = run("echo a; exit 3; echo b");
        assert_eq!(out, ReplOutcome::Exit(3));
        assert_eq!(sink.stdout, "a\n");
    }

    #[test]
    fn bare_exit_uses_last_status() {
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        repl.run_line("false", &mut sink);
        assert_eq!(repl.run_line("exit", &mut sink), ReplOutcome::Exit(1));
    }

    #[test]
    fn exit_with_bad_number_is_status_two() {
        let (out, sink) = run("exit soon");
        assert_eq!(out, ReplOutcome::Exit(2));
        assert!(sink.stderr.contains("numeric argument required"));
    }

    #[test]
    fn unknown_command_is_127_with_message() {
        let (out, sink) = run("ls -l");
        assert_eq!(out, ReplOutcome::Continue(127));
        assert_eq!(sink.stderr, "tpt-k: ls: not supported yet\n");
        assert!(sink.stdout.is_empty());
    }

    #[test]
    fn background_job_is_listed_and_not_run() {
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        repl.run_line("echo hi &", &mut sink);
        assert_eq!(sink.stdout, "[1]\n");
        // The echo was recorded as a job, not executed.
        assert_eq!(repl.jobs().get(1).unwrap().command, "echo hi &");
        let mut sink = Buffers::default();
        repl.run_line("jobs", &mut sink);
        assert!(sink.stdout.starts_with("[1] Running"));
        assert!(sink.stdout.contains("echo hi &"));
        assert!(!sink.stdout.contains("\nhi\n"));
    }

    #[test]
    fn fg_and_bg_on_missing_job_report_an_error() {
        let (out, sink) = run("fg %7");
        assert_eq!(out, ReplOutcome::Continue(1));
        assert_eq!(sink.stderr, "tpt-k: fg: no such job\n");
        let (out, sink) = run("bg");
        assert_eq!(out, ReplOutcome::Continue(1));
        assert_eq!(sink.stderr, "tpt-k: bg: no such job\n");
        let (_, sink) = run("fg notajob");
        assert_eq!(sink.stderr, "tpt-k: fg: bad job spec\n");
    }

    #[test]
    fn fg_and_bg_move_a_background_job() {
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        repl.run_line("true &", &mut sink);
        let mut sink = Buffers::default();
        repl.run_line("fg %1", &mut sink);
        assert_eq!(sink.stdout, "true &\n");
        assert!(!repl.jobs().get(1).unwrap().background);
        let mut sink = Buffers::default();
        repl.run_line("bg %1", &mut sink);
        assert!(repl.jobs().get(1).unwrap().background);
    }

    #[test]
    fn redirection_is_refused_not_run() {
        let (out, sink) = run("echo hi > out.txt");
        assert_eq!(out, ReplOutcome::Continue(1));
        assert_eq!(sink.stderr, "tpt-k: redirections are not supported yet\n");
        assert!(sink.stdout.is_empty());
    }

    #[test]
    fn pipeline_is_refused_with_an_error() {
        let (_, sink) = run("echo a | echo b");
        assert_eq!(sink.stderr, "tpt-k: pipelines are not supported yet\n");
    }

    #[test]
    fn syntax_error_is_reported_with_status_two() {
        let (out, sink) = run("echo 'unclosed");
        assert_eq!(out, ReplOutcome::Continue(2));
        assert!(sink.stderr.starts_with("tpt-k: syntax error"));
    }

    #[test]
    fn run_reader_stops_at_exit_and_prompts() {
        let script = "echo one\nfalse\nexit 4\necho never\n";
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        let code = run_reader(&mut repl, script.as_bytes(), &mut sink, true).unwrap();
        assert_eq!(code, 4);
        assert_eq!(sink.stdout, "> one\n> > ");
    }

    #[test]
    fn run_reader_without_exit_returns_last_status() {
        let mut repl = Repl::new();
        let mut sink = Buffers::default();
        let code = run_reader(&mut repl, "false\n".as_bytes(), &mut sink, false).unwrap();
        assert_eq!(code, 1);
    }
}
