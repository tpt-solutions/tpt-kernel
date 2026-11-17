// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tpt-k-build`: plans the cargo commands for the tpt-k workspace, checks the
//! assurance graph and the machine, and builds and boots the image.
//!
//! `plan` and `check` only print the cargo commands, and `assure` only reads
//! files. `image --run` and `run` run cargo and QEMU, from the repo root.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use tpt_k_toolchain::assurance;
use tpt_k_toolchain::doctor::{self, Check, Finding, Os, Probe};
use tpt_k_toolchain::runner::{self, QemuRun, Serial};
use tpt_k_toolchain::{crates_from_metadata, image_steps, plan, CrateInfo, PlanError, Subcommand};

const USAGE: &str = "\
tpt-k-build: plan the cargo commands for the tpt-k workspace.

USAGE:
    tpt-k-build <plan|check|image|run|assure|doctor|help> [--metadata <file|->] [--fixture <a,b,...>]
                        [--graph <file>] [--root <dir>] [--report <file>]

COMMANDS:
    plan     print `cargo build` commands for each crate, grouped by target
    check    print `cargo check` commands for each crate, grouped by target
    image    print the steps that would assemble the boot image; with --run,
             build the kernel and loader and stage the EFI System Partition
             under target/tpt-k/esp (run from the repo root)
    run      do `image --run`, then boot it in QEMU with the serial port on
             this terminal
    assure   check the assurance graph; exit 1 if the gate fails
    doctor   check the toolchain, targets, QEMU, OVMF, sibling repos and the
             git hooks path, and print the fix for each problem; run it from
             the repo root. Exit 1 if a required check fails
    help     print this message

OPTIONS:
    --metadata <file|->  read `cargo metadata --no-deps --format-version 1`
                         output from a file, or from stdin with `-`
    --fixture <a,b,...>  use these crate names instead of metadata
    --graph <file>       assurance graph (default docs/assurance/graph.json)
    --root <dir>         repository root the evidence paths are relative to
                         (default: the current directory)
    --report <file>      also write the Markdown report to this file

`plan` and `check` never run cargo: the plan is printed for you to run, or to
pipe to a shell. `image` prints the steps only, unless `--run` is given. Only
`image --run` and `run` run cargo and QEMU. `assure` runs no cargo either: it
checks that the evidence exists, and it does not run the tests.";

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
        Err(Failure::Report(text)) => {
            // A report with failures is still output: print it on stdout.
            let _ = io::stdout().lock().write_all(text.as_bytes());
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
    /// A report that found a problem, such as the doctor's. The text is the
    /// report itself, printed on stdout.
    Report(String),
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
        "image" if options.run => assemble_image(false),
        "run" => assemble_image(true),
        "image" => {
            let mut text = String::from("boot image steps (not run; add --run to build them):\n");
            for step in image_steps() {
                text.push_str(&step);
                text.push('\n');
            }
            Ok(text)
        }
        "plan" => Ok(planned(&options, Subcommand::Build)?.render()),
        "check" => Ok(planned(&options, Subcommand::Check)?.render()),
        "assure" => assure(&options),
        "doctor" => doctor_report(),
        other => Err(Failure::Usage(format!("unknown command `{other}`"))),
    }
}

/// Builds the kernel and the loader, stages the EFI System Partition under
/// `target/tpt-k/esp`, and, if `launch`, boots it in QEMU with the serial port on
/// this terminal's standard streams. Runs from the repo root.
fn assemble_image(launch: bool) -> Result<String, Failure> {
    let root = std::env::current_dir().map_err(|e| Failure::Io(e.to_string()))?;
    if !root.join("rust-toolchain.toml").is_file() {
        return Err(Failure::Gate(
            "run from the repo root (rust-toolchain.toml not found here)".to_string(),
        ));
    }
    let plan = runner::plan_image(&root);
    for step in &plan.builds {
        eprintln!(
            "tpt-k-build: building the {} (cargo {})",
            step.what,
            step.args.join(" ")
        );
        let status = Command::new("cargo")
            .args(&step.args)
            .current_dir(&root)
            .status()
            .map_err(|e| Failure::Io(format!("cannot run cargo: {e}")))?;
        if !status.success() {
            return Err(Failure::Gate(format!("the {} build failed", step.what)));
        }
    }
    for file in &plan.esp {
        let dest = runner::esp_target(&plan, file.dest);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).map_err(|e| Failure::Io(format!("{}: {e}", dir.display())))?;
        }
        fs::copy(&file.src, &dest)
            .map_err(|e| Failure::Io(format!("copy {}: {e}", file.src.display())))?;
    }
    let mut text = format!("image staged in {}\n", runner::esp_dir(&plan).display());
    if !launch {
        return Ok(text);
    }

    let os = Os::current();
    let qemu = find_qemu(os)?;
    let ovmf_code = find_firmware("OVMF_CODE", runner::ovmf_candidates(os))?;
    let ovmf_template = find_firmware("OVMF_VARS", runner::ovmf_vars_candidates(os))?;
    // OVMF writes its variables, so each run gets its own copy of the template.
    fs::create_dir_all(&plan.out).map_err(|e| Failure::Io(e.to_string()))?;
    let ovmf_vars = plan.out.join("vars.fd");
    fs::copy(&ovmf_template, &ovmf_vars)
        .map_err(|e| Failure::Io(format!("copy {}: {e}", ovmf_template.display())))?;
    let launch = QemuRun {
        qemu,
        ovmf_code,
        ovmf_vars,
        esp: runner::esp_dir(&plan),
        serial: Serial::Stdio,
    };
    eprintln!("tpt-k-build: booting in QEMU (serial on this terminal; Ctrl-A X quits)");
    let status = Command::new(&launch.qemu)
        .args(runner::qemu_args(&launch))
        .status()
        .map_err(|e| Failure::Io(format!("cannot start QEMU: {e}")))?;
    text.push_str(&match status.code() {
        Some(code) => format!("QEMU exited with status {code}\n"),
        None => "QEMU was stopped by a signal\n".to_string(),
    });
    Ok(text)
}

/// The QEMU binary: `QEMU` if set, then `qemu-system-x86_64` on PATH, then the
/// default Windows install.
fn find_qemu(os: Os) -> Result<PathBuf, Failure> {
    if let Ok(path) = std::env::var("QEMU") {
        return Ok(PathBuf::from(path));
    }
    if command_output("qemu-system-x86_64", &["--version"]).is_some() {
        return Ok(PathBuf::from("qemu-system-x86_64"));
    }
    if os == Os::Windows && PathBuf::from(runner::WINDOWS_QEMU).is_file() {
        return Ok(PathBuf::from(runner::WINDOWS_QEMU));
    }
    Err(Failure::Gate(
        "QEMU not found; install it, or set QEMU to qemu-system-x86_64".to_string(),
    ))
}

/// A firmware file: the path in environment variable `var` if set, else the
/// first candidate that exists.
fn find_firmware(var: &str, candidates: &[&str]) -> Result<PathBuf, Failure> {
    if let Ok(path) = std::env::var(var) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Failure::Gate(format!(
            "{var} is set, but the file is missing"
        )));
    }
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| Failure::Gate(format!("no firmware found; set {var}")))
}

/// Probes this machine for each check, and renders the report.
fn doctor_report() -> Result<String, Failure> {
    let os = Os::current();
    let findings = vec![
        Finding {
            check: Check::RustToolchain,
            probe: probe_rust(),
        },
        Finding {
            check: Check::Targets,
            probe: probe_targets(),
        },
        Finding {
            check: Check::Qemu,
            probe: probe_qemu(os),
        },
        Finding {
            check: Check::Ovmf,
            probe: probe_ovmf(os),
        },
        Finding {
            check: Check::SiblingRepos,
            probe: probe_siblings(),
        },
        Finding {
            check: Check::HooksPath,
            probe: probe_hooks(),
        },
    ];
    let (text, ok) = doctor::render(&findings, os);
    if ok {
        Ok(text)
    } else {
        Err(Failure::Report(text))
    }
}

/// Output of a command, or `None` if it could not be run or failed.
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn probe_rust() -> Probe {
    let Some(text) = command_output("rustc", &["--version"]) else {
        return Probe::Missing("rustc is not on PATH".to_string());
    };
    match doctor::parse_rustc_version(&text) {
        Some(v) if v == doctor::REQUIRED_RUST => Probe::Ok(format!("rustc {v}")),
        Some(v) => Probe::Wrong(format!(
            "rustc {v}, the repo pins {}",
            doctor::REQUIRED_RUST
        )),
        None => Probe::Missing(format!("unexpected output from rustc: {}", text.trim())),
    }
}

fn probe_targets() -> Probe {
    let Some(text) = command_output("rustup", &["target", "list", "--installed"]) else {
        return Probe::Missing(
            "rustup is not on PATH, so the targets cannot be checked".to_string(),
        );
    };
    let missing = doctor::missing_targets(&text);
    if missing.is_empty() {
        Probe::Ok(format!("{} installed", doctor::REQUIRED_TARGETS.join(", ")))
    } else {
        Probe::Missing(format!("not installed: {}", missing.join(", ")))
    }
}

fn probe_qemu(os: Os) -> Probe {
    if let Ok(path) = std::env::var("QEMU") {
        return if PathBuf::from(&path).is_file() {
            Probe::Ok(format!("QEMU={path}"))
        } else {
            Probe::Wrong(format!("QEMU={path} is not a file"))
        };
    }
    if command_output("qemu-system-x86_64", &["--version"]).is_some() {
        return Probe::Ok("qemu-system-x86_64 on PATH".to_string());
    }
    if os == Os::Windows {
        let default = r"C:\Program Files\qemu\qemu-system-x86_64.exe";
        if PathBuf::from(default).is_file() {
            return Probe::Ok(default.to_string());
        }
    }
    Probe::Missing(
        "qemu-system-x86_64 not found on PATH or in its default install folder".to_string(),
    )
}

fn probe_ovmf(os: Os) -> Probe {
    if let Ok(path) = std::env::var("OVMF_CODE") {
        return if PathBuf::from(&path).is_file() {
            Probe::Ok(format!("OVMF_CODE={path}"))
        } else {
            Probe::Wrong(format!("OVMF_CODE={path} is not a file"))
        };
    }
    // The same places scripts/qemu-smoke.sh looks.
    let candidates: &[&str] = match os {
        Os::Windows => &[r"C:\Program Files\qemu\share\edk2-x86_64-code.fd"],
        Os::Linux => &[
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        ],
    };
    match candidates.iter().find(|p| PathBuf::from(p).is_file()) {
        Some(p) => Probe::Ok((*p).to_string()),
        None => Probe::Missing("no OVMF_CODE firmware found in the usual places".to_string()),
    }
}

fn probe_siblings() -> Probe {
    let Ok(cwd) = std::env::current_dir() else {
        return Probe::Missing("cannot read the current directory".to_string());
    };
    let Some(parent) = cwd.parent() else {
        return Probe::Missing("run the doctor from the repo root".to_string());
    };
    if !cwd.join("rust-toolchain.toml").is_file() {
        return Probe::Missing("run the doctor from the repo root".to_string());
    }
    let absent: Vec<&str> = doctor::SIBLING_REPOS
        .iter()
        .copied()
        .filter(|name| !parent.join(name).is_dir())
        .collect();
    if absent.is_empty() {
        Probe::Ok(format!(
            "all {} present next to this repo",
            doctor::SIBLING_REPOS.len()
        ))
    } else {
        Probe::Missing(format!(
            "not found next to this repo: {}",
            absent.join(", ")
        ))
    }
}

fn probe_hooks() -> Probe {
    let value =
        command_output("git", &["config", "--get", "core.hooksPath"]).map(|v| v.trim().to_string());
    match value.as_deref() {
        Some(doctor::HOOKS_PATH) => Probe::Ok(format!("core.hooksPath = {}", doctor::HOOKS_PATH)),
        Some(other) => Probe::Wrong(format!("core.hooksPath is {other:?}")),
        None => {
            Probe::Missing("core.hooksPath is not set, so the pre-push hook is off".to_string())
        }
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
    /// `image --run`: build and stage the image, instead of only printing the steps.
    run: bool,
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
            "--run" => options.run = true,
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
        assert!(text.contains("never run cargo"));
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
