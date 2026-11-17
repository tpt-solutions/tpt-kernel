// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tpt-k-build`: prints the cargo commands for the tpt-k workspace.
//!
//! This version does not run cargo. It plans. Feed it the output of
//! `cargo metadata --no-deps --format-version 1` with `--metadata`, or name
//! the crates with `--fixture`.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use std::path::Path;

use tpt_k_toolchain::assurance;
use tpt_k_toolchain::{crates_from_metadata, image_steps, plan, CrateInfo, PlanError, Subcommand};

const USAGE: &str = "\
tpt-k-build: plan the cargo commands for the tpt-k workspace.

USAGE:
    tpt-k-build <plan|check|image|assure|help> [--metadata <file|->] [--fixture <a,b,...>]
                        [--graph <file>] [--root <dir>] [--report <file>]

COMMANDS:
    plan     print `cargo build` commands for each crate, grouped by target
    check    print `cargo check` commands for each crate, grouped by target
    image    print the steps that would assemble the boot image
    assure   check the assurance graph; exit 1 if the gate fails
    help     print this message

OPTIONS:
    --metadata <file|->  read `cargo metadata --no-deps --format-version 1`
                         output from a file, or from stdin with `-`
    --fixture <a,b,...>  use these crate names instead of metadata
    --graph <file>       assurance graph (default docs/assurance/graph.json)
    --root <dir>         repository root the evidence paths are relative to
                         (default: the current directory)
    --report <file>      also write the Markdown report to this file

This version never runs cargo. The plan is printed for you to run, or to
pipe to a shell. `image` prints the steps only. `assure` runs no cargo either:
it checks that the evidence exists, and it does not run the tests.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(text) => {
            let mut out = io::stdout().lock();
            if out.write_all(text.as_bytes()).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => {
            eprintln!("tpt-k-build: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Plan(err)) => {
            eprintln!("tpt-k-build: {err}");
            ExitCode::FAILURE
        }
        Err(Failure::Io(err)) => {
            eprintln!("tpt-k-build: {err}");
            ExitCode::FAILURE
        }
        Err(Failure::Gate(message)) => {
            eprintln!("tpt-k-build: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Why the command could not produce output.
#[derive(Debug)]
enum Failure {
    /// The arguments are wrong. Exit status 2, with the usage text.
    Usage(String),
    /// A crate name or the metadata text is not valid.
    Plan(PlanError),
    /// Reading a file or stdin failed.
    Io(String),
    /// The assurance graph is malformed, or the gate failed. The message says why.
    Gate(String),
}

impl From<PlanError> for Failure {
    fn from(err: PlanError) -> Self {
        Self::Plan(err)
    }
}

/// Runs one command line and returns what it prints.
fn run(args: &[String]) -> Result<String, Failure> {
    let command = match args.first().map(String::as_str) {
        None => return Err(Failure::Usage("no command given".to_string())),
        Some("help" | "--help" | "-h") => return Ok(format!("{USAGE}\n")),
        Some(cmd) => cmd,
    };
    let options = parse_options(&args[1..])?;

    match command {
        "image" => {
            let mut text = String::from("boot image steps (not run):\n");
            for step in image_steps() {
                text.push_str(&step);
                text.push('\n');
            }
            Ok(text)
        }
        "plan" => Ok(planned(&options, Subcommand::Build)?.render()),
        "check" => Ok(planned(&options, Subcommand::Check)?.render()),
        "assure" => assure(&options),
        other => Err(Failure::Usage(format!("unknown command `{other}`"))),
    }
}

/// The crate list from the options, planned for `subcommand`.
fn planned(options: &Options, subcommand: Subcommand) -> Result<tpt_k_toolchain::Plan, Failure> {
    let crates = crate_list(options)?;
    Ok(plan(&crates, subcommand))
}

/// The crates named by `--fixture`, or by `--metadata`.
fn crate_list(options: &Options) -> Result<Vec<CrateInfo>, Failure> {
    if let Some(names) = &options.fixture {
        return names
            .split(',')
            .filter(|n| !n.is_empty())
            .map(|n| CrateInfo::new(n).map_err(Failure::from))
            .collect();
    }
    match options.metadata.as_deref() {
        Some("-") => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| Failure::Io(format!("reading stdin: {e}")))?;
            Ok(crates_from_metadata(&text)?)
        }
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| Failure::Io(format!("reading `{path}`: {e}")))?;
            Ok(crates_from_metadata(&text)?)
        }
        None => Err(Failure::Usage(
            "give --fixture <names> or --metadata <file|->".to_string(),
        )),
    }
}

/// Checks the assurance graph. Prints the Markdown report on success. On
/// failure, the problems are returned as the error. The report file, if one is
/// named, is written either way, so CI can keep it.
fn assure(options: &Options) -> Result<String, Failure> {
    let graph_path = options
        .graph
        .as_deref()
        .unwrap_or("docs/assurance/graph.json");
    let root = options.root.as_deref().unwrap_or(".");
    let text = std::fs::read_to_string(graph_path)
        .map_err(|e| Failure::Io(format!("reading `{graph_path}`: {e}")))?;
    let claims = assurance::parse(&text).map_err(|e| Failure::Gate(format!("graph: {e}")))?;
    let report = assurance::evaluate(&claims, Path::new(root));
    let markdown = report.render();
    if let Some(path) = &options.report {
        std::fs::write(path, &markdown)
            .map_err(|e| Failure::Io(format!("writing `{path}`: {e}")))?;
    }
    if report.passed() {
        Ok(markdown)
    } else {
        let mut message = String::from("assurance gate failed:\n");
        for problem in report.problems() {
            message.push_str("  ");
            message.push_str(&problem);
            message.push('\n');
        }
        Err(Failure::Gate(message))
    }
}

#[derive(Default)]
struct Options {
    metadata: Option<String>,
    fixture: Option<String>,
    graph: Option<String>,
    root: Option<String>,
    report: Option<String>,
}

fn next_value<'a>(
    iter: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<String, Failure> {
    iter.next()
        .cloned()
        .ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))
}

fn parse_options(args: &[String]) -> Result<Options, Failure> {
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--metadata" => {
                let value = iter
                    .next()
                    .ok_or_else(|| Failure::Usage("--metadata needs a value".to_string()))?;
                options.metadata = Some(value.clone());
            }
            "--fixture" => {
                let value = iter
                    .next()
                    .ok_or_else(|| Failure::Usage("--fixture needs a value".to_string()))?;
                options.fixture = Some(value.clone());
            }
            "--graph" => options.graph = Some(next_value(&mut iter, "--graph")?),
            "--root" => options.root = Some(next_value(&mut iter, "--root")?),
            "--report" => options.report = Some(next_value(&mut iter, "--report")?),
            other => return Err(Failure::Usage(format!("unknown option `{other}`"))),
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_prints_usage_and_says_cargo_is_not_run() {
        let text = run(&args(&["help"])).unwrap();
        assert!(text.contains("USAGE"));
        assert!(text.contains("never runs cargo"));
    }

    #[test]
    fn plan_from_fixture() {
        let text = run(&args(&["plan", "--fixture", "tpt-k-boot,tpt-k-ipc"])).unwrap();
        assert_eq!(
            text,
            "cargo build -p tpt-k-boot --target x86_64-unknown-uefi\n\
             cargo build -p tpt-k-ipc\n"
        );
    }

    #[test]
    fn check_uses_check() {
        let text = run(&args(&["check", "--fixture", "tpt-k-kernel"])).unwrap();
        assert_eq!(
            text,
            "cargo check -p tpt-k-kernel --target x86_64-unknown-none\n"
        );
    }

    #[test]
    fn plan_without_crate_list_is_a_usage_error() {
        assert!(matches!(run(&args(&["plan"])), Err(Failure::Usage(_))));
    }

    #[test]
    fn unknown_layer_in_fixture_is_a_plan_error() {
        assert!(matches!(
            run(&args(&["plan", "--fixture", "serde"])),
            Err(Failure::Plan(PlanError::UnknownLayer(_)))
        ));
    }

    #[test]
    fn image_lists_steps() {
        let text = run(&args(&["image"])).unwrap();
        assert!(text.contains("not run"));
        assert!(text.contains("BOOTX64.EFI"));
    }

    #[test]
    fn unknown_command_is_a_usage_error() {
        assert!(matches!(run(&args(&["build"])), Err(Failure::Usage(_))));
    }
}
