// SPDX-License-Identifier: MIT OR Apache-2.0
//! `cargo xtask`: the repo's build, check, smoke-test and dependency-fetch tasks,
//! in Rust, so they run the same on Windows and Linux. The shell scripts in
//! `scripts/` call this binary and add nothing of their own.
//!
//! Run from anywhere in the repo:
//!
//! ```text
//! cargo xtask check          fmt, clippy, tests, the assurance gate, the license gate
//! cargo xtask build-kernel   the kernel with its linker script
//! cargo xtask qemu-smoke     build, boot under QEMU and OVMF, check the serial output
//! cargo xtask fetch-deps     clone or fast-forward the sibling TPT repos
//! ```
//!
//! `qemu-smoke` honours the environment variables of `scripts/qemu-smoke.sh`:
//! `QEMU`, `OVMF_CODE`, `OVMF_VARS`, `OUT` and `TIMEOUT`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use tpt_k_toolchain::doctor::{Os, SIBLING_REPOS};
use tpt_k_toolchain::runner::{self, BuildStep, ImagePlan, QemuRun, Serial};

/// The debug-exit status the kernel writes when its smoke test passes.
const EXPECTED_QEMU_STATUS: i32 = 33;

/// Lines the serial output must contain, in any order.
const EXPECTED_TEXT: [&str; 27] = [
    "caps: begin",
    "caps: end",
    "Hello tpt-k",
    "handoff: rsdp at 0x",
    "handoff: framebuffer ",
    "ioapic: rtc irqs=2",
    "irq: driver woke",
    "user: fault vector 14",
    "paging: remap ok",
    "hello from ring 3",
    "apic: timer",
    "handles: process tables are isolated",
    "handles: grant attenuates rights",
    "errno: failures are negated errno values",
    "mem: freed object returned its frames",
    "mem: revoking the last handle reclaimed the object",
    "user: exit 10",
    "user: exit 2",
    "user: exit 12648430",
    "spawn: child in slot",
    "spawn: image child in slot",
    "user: exit 7",
    "user: exit 90",
    "user: exit 5",
    "trap 3 (breakpoint)",
    "ipc: server got 1 2 3 4",
    "preempt: ticks=",
];

/// The final line the kernel prints when its scheduler is done.
const FINAL_TEXT: &str = "scheduler: ok";

/// Licenses the CVE and license gate accepts, as `scripts/check.sh` lists them.
const LICENSES: &str =
    "MIT,Unicode-3.0,Unicode-DFS-2016,BSD-2-Clause,BSD-3-Clause,ISC,Zlib,Unlicense,CC0-1.0,MIT-0,BSL-1.0";

/// Where the sibling repos are cloned from.
const ORG_URL: &str = "https://github.com/tpt-solutions";

/// Git options for clone and pull. Some sibling repos have test-vector file names
/// longer than Windows' 260-character limit, which git can check out only with
/// long paths enabled. The option is harmless on Linux.
const LONG_PATHS: [&str; 2] = ["-c", "core.longpaths=true"];

/// Packages that are built for their own targets and are left out of the host
/// checks.
const BARE_METAL_EXCLUDES: [&str; 4] = ["--exclude", "tpt-k-boot", "--exclude", "tpt-k-kernel"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = match repo_root() {
        Ok(root) => root,
        Err(message) => return fail(&message),
    };
    let result = match args.first().map(String::as_str) {
        Some("check") => check(&root),
        Some("build-kernel") => build_kernel(&root),
        Some("qemu-smoke") => qemu_smoke(&root),
        Some("fetch-deps") => fetch_deps(&root, args.get(1).map(String::as_str)),
        Some("help" | "--help" | "-h") | None => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

const USAGE: &str = "\
usage: cargo xtask <task>

tasks:
    check          fmt, clippy, host tests, the assurance gate, the license gate
    build-kernel   build the kernel with its linker script
    qemu-smoke     build, boot under QEMU and OVMF, and check the serial output
    fetch-deps     clone or fast-forward the sibling TPT repos (argument: target dir)";

fn fail(message: &str) -> ExitCode {
    eprintln!("xtask: {message}");
    ExitCode::FAILURE
}

/// The repo root, from this crate's location: `crates/<this crate>/`.
fn repo_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .map_err(|e| format!("cannot find the repo root: {e}"))
}

/// Runs `program` with `args` in `dir`, with its output on this terminal.
fn run(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    eprintln!("xtask: {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program} {}` failed", args.join(" ")))
    }
}

/// Runs one cargo step from a plan.
fn run_build(root: &Path, step: &BuildStep) -> Result<(), String> {
    let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
    run(root, "cargo", &args).map_err(|e| format!("the {} build failed: {e}", step.what))
}

fn check(root: &Path) -> Result<(), String> {
    run(root, "cargo", &["fmt", "--all", "--", "--check"])?;

    let mut clippy = vec!["clippy", "--workspace"];
    clippy.extend(BARE_METAL_EXCLUDES);
    clippy.extend(["--all-targets", "--", "-D", "warnings"]);
    run(root, "cargo", &clippy)?;

    let mut test = vec!["test", "--workspace"];
    test.extend(BARE_METAL_EXCLUDES);
    run(root, "cargo", &test)?;

    // The assurance gate. Its report goes to docs/assurance/report.md.
    run(
        root,
        "cargo",
        &[
            "run",
            "-q",
            "-p",
            "tpt-k-toolchain",
            "--bin",
            "tpt-k-build",
            "--",
            "assure",
            "--root",
            ".",
            "--report",
            "docs/assurance/report.md",
        ],
    )?;

    if command_succeeds("tpt-cargo-vitals", &["--version"]) {
        run(
            root,
            "tpt-cargo-vitals",
            &[
                "check",
                "--path",
                ".",
                "--enforce-license",
                LICENSES,
                "--ci-mode",
            ],
        )?;
    } else {
        eprintln!("xtask: tpt-cargo-vitals not on PATH: license gate skipped");
    }
    println!("check: all local gates passed");
    Ok(())
}

fn build_kernel(root: &Path) -> Result<(), String> {
    let plan = runner::plan_image(root);
    run_build(root, &plan.builds[0])
}

fn qemu_smoke(root: &Path) -> Result<(), String> {
    let os = Os::current();
    let plan = runner::plan_image(root);
    for step in &plan.builds {
        run_build(root, step)?;
    }

    let out = env::var("OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("target").join("qemu-smoke"));
    stage_esp(&plan, &out)?;
    let qemu = find_qemu(os)?;
    let ovmf_code = firmware("OVMF_CODE", runner::ovmf_candidates(os))?;
    let ovmf_vars_template = firmware("OVMF_VARS", runner::ovmf_vars_candidates(os))?;
    let ovmf_vars = out.join("vars.fd");
    fs::copy(&ovmf_vars_template, &ovmf_vars).map_err(|e| format!("copy OVMF vars: {e}"))?;
    let timeout = env::var("TIMEOUT")
        .ok()
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(60);
    let launch = QemuRun {
        qemu,
        ovmf_code,
        ovmf_vars,
        // Boot the copy that `stage_esp` just wrote, under `out`, not the
        // `target/tpt-k/esp` that only `cargo xtask image` stages.
        esp: out.join("esp"),
        serial: Serial::Stdio,
        // Host time. The instruction counter is for `tpt-k-build replay`.
        icount: false,
    };
    eprintln!("xtask: booting (timeout {timeout}s)");
    let outcome = runner::capture(&launch, Duration::from_secs(timeout))?;
    eprintln!("xtask: QEMU exit status {:?}", outcome.status);
    eprintln!(
        "--- serial output ---\n{}--- end serial output ---",
        outcome.transcript
    );
    check_serial(outcome.status, &outcome.transcript)?;
    println!("qemu-smoke: PASS");
    Ok(())
}

/// Copies the ESP files into `out/esp`, the layout the loader expects.
fn stage_esp(plan: &ImagePlan, out: &Path) -> Result<(), String> {
    let esp = out.join("esp");
    for file in &plan.esp {
        let dest = esp.join(file.dest.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::copy(&file.src, &dest).map_err(|e| format!("copy {}: {e}", file.src.display()))?;
    }
    Ok(())
}

/// Starts QEMU and waits for it. Returns its exit status, or fails after
/// `timeout`, when QEMU is killed.
/// Checks the QEMU status and the serial transcript, as `scripts/qemu-smoke.sh`
/// did. Returns the first check that fails.
fn check_serial(status: Option<i32>, serial: &str) -> Result<(), String> {
    if status != Some(EXPECTED_QEMU_STATUS) {
        return Err(format!(
            "expected QEMU status {EXPECTED_QEMU_STATUS}, got {status:?}"
        ));
    }
    for text in EXPECTED_TEXT {
        if !serial.contains(text) {
            return Err(format!("serial output lacks '{text}'"));
        }
    }
    if !serial.contains(FINAL_TEXT) {
        return Err(format!("serial output lacks '{FINAL_TEXT}'"));
    }
    // The first user process exits with 1. "user: exit 1" is also a prefix of
    // "user: exit 10", so the line must end there.
    if !serial
        .lines()
        .any(|line| line.trim_end().ends_with("user: exit 1"))
    {
        return Err("process 1 did not exit with its number".to_string());
    }
    if !has_nonzero_count(serial, "memory: ", " free frames") {
        return Err("no free-frame count from the memory map".to_string());
    }
    let a = serial.matches("<A>").count();
    let b = serial.matches("<B>").count();
    if a != 3 || b != 3 {
        return Err(format!("worker prints A={a} B={b}, expected 3 and 3"));
    }
    if !has_nonzero_count(serial, "timer switches=", "") {
        return Err("no timer-driven preemption in the serial output".to_string());
    }
    Ok(())
}

/// True if `serial` has `prefix`, then a number above zero, then `suffix`.
fn has_nonzero_count(serial: &str, prefix: &str, suffix: &str) -> bool {
    serial.match_indices(prefix).any(|(at, _)| {
        let rest = &serial[at + prefix.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        !digits.is_empty()
            && digits.bytes().any(|b| b != b'0')
            && rest[digits.len()..].starts_with(suffix)
    })
}

/// The QEMU binary, as `runner` finds it: `QEMU`, then PATH, then the default
/// Windows install.
fn find_qemu(os: Os) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("QEMU") {
        return Ok(PathBuf::from(path));
    }
    if command_succeeds("qemu-system-x86_64", &["--version"]) {
        return Ok(PathBuf::from("qemu-system-x86_64"));
    }
    if os == Os::Windows && Path::new(runner::WINDOWS_QEMU).is_file() {
        return Ok(PathBuf::from(runner::WINDOWS_QEMU));
    }
    Err("qemu-system-x86_64 not found; set QEMU".to_string())
}

/// A firmware file: the path in `var` if set, else the first candidate present.
fn firmware(var: &str, candidates: &[&str]) -> Result<PathBuf, String> {
    if let Ok(path) = env::var(var) {
        return Ok(PathBuf::from(path));
    }
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no firmware found; set {var}"))
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn fetch_deps(root: &Path, target: Option<&str>) -> Result<(), String> {
    let parent = match target {
        Some(dir) => PathBuf::from(dir),
        None => root
            .parent()
            .ok_or("the repo has no parent folder")?
            .to_path_buf(),
    };
    let (mut ok, mut skipped, mut failed) = (0, 0, 0);
    for name in SIBLING_REPOS {
        match sync_repo(&parent, name) {
            Sync::Ok(message) => {
                println!("[ok]   {name}: {message}");
                ok += 1;
            }
            Sync::Skipped(message) => {
                println!("[skip] {name}: {message}");
                skipped += 1;
            }
            Sync::Failed(message) => {
                println!("[fail] {name}: {message}");
                failed += 1;
            }
        }
    }
    println!("\ndone: {ok} ok, {skipped} skipped, {failed} failed");
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} repo(s) failed"))
    }
}

enum Sync {
    Ok(&'static str),
    Skipped(String),
    Failed(String),
}

/// Clones `name` into `parent`, or fast-forwards it. A repo with local changes is
/// not touched, and a folder that is not a git repo is reported.
fn sync_repo(parent: &Path, name: &str) -> Sync {
    let dir = parent.join(name);
    if dir.join(".git").is_dir() {
        let status = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["status", "--porcelain"])
            .output();
        match status {
            Ok(out) if out.stdout.is_empty() => {}
            Ok(_) => return Sync::Skipped("local changes present, not pulling".to_string()),
            Err(e) => return Sync::Failed(format!("git: {e}")),
        }
        return match Command::new("git")
            .args(LONG_PATHS)
            .args(["-C"])
            .arg(&dir)
            .args(["pull", "--ff-only", "--quiet"])
            .status()
        {
            Ok(s) if s.success() => Sync::Ok("up to date"),
            _ => Sync::Failed("pull failed".to_string()),
        };
    }
    if dir.exists() {
        return Sync::Skipped(format!("{} exists but is not a git repo", dir.display()));
    }
    let url = format!("{ORG_URL}/{name}.git");
    let remote = Command::new("git")
        .args(["ls-remote", "--exit-code", &url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !remote.is_ok_and(|s| s.success()) {
        return Sync::Skipped(format!("no remote at {url} yet (local-only repo)"));
    }
    match Command::new("git")
        .args(LONG_PATHS)
        .args(["clone", "--quiet", &url])
        .arg(&dir)
        .status()
    {
        Ok(s) if s.success() => Sync::Ok("cloned"),
        _ => Sync::Failed("clone failed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transcript that passes every check.
    fn passing() -> String {
        let mut text = String::new();
        for line in EXPECTED_TEXT {
            text.push_str(line);
            text.push('\n');
        }
        text.push_str("memory: 51993 free frames\n");
        text.push_str("user: exit 1\n");
        text.push_str("<A><B><A><B><A><B>\n");
        text.push_str("timer switches=164\n");
        text.push_str(FINAL_TEXT);
        text.push('\n');
        text
    }

    #[test]
    fn a_passing_transcript_passes() {
        assert_eq!(check_serial(Some(33), &passing()), Ok(()));
    }

    #[test]
    fn wrong_status_fails_first() {
        let err = check_serial(Some(1), &passing()).unwrap_err();
        assert!(err.contains("expected QEMU status 33"));
    }

    #[test]
    fn a_missing_line_is_named() {
        let text = passing().replace("Hello tpt-k", "Hello");
        let err = check_serial(Some(33), &text).unwrap_err();
        assert!(err.contains("'Hello tpt-k'"), "{err}");
    }

    #[test]
    fn user_exit_one_must_end_the_line() {
        // "user: exit 10" is present, but "user: exit 1" alone is not.
        let text = passing().replace("user: exit 1\n", "user: exit 10\n");
        let err = check_serial(Some(33), &text).unwrap_err();
        assert!(err.contains("process 1 did not exit"), "{err}");
    }

    #[test]
    fn worker_counts_must_be_three_each() {
        let text = passing().replace("<A><B><A><B><A><B>", "<A><B><A><B>");
        assert!(check_serial(Some(33), &text).unwrap_err().contains("A=2"));
    }

    #[test]
    fn free_frames_and_timer_counts_need_a_nonzero_number() {
        let text = passing().replace("memory: 51993", "memory: 0");
        assert!(check_serial(Some(33), &text)
            .unwrap_err()
            .contains("free-frame"));
        let text = passing().replace("timer switches=164", "timer switches=0");
        assert!(check_serial(Some(33), &text)
            .unwrap_err()
            .contains("preemption"));
    }

    #[test]
    fn nonzero_count_parser() {
        assert!(has_nonzero_count(
            "x memory: 12 free frames",
            "memory: ",
            " free frames"
        ));
        assert!(!has_nonzero_count(
            "memory: 0 free frames",
            "memory: ",
            " free frames"
        ));
        assert!(!has_nonzero_count(
            "memory: 12 frames",
            "memory: ",
            " free frames"
        ));
        assert!(!has_nonzero_count(
            "memory: free frames",
            "memory: ",
            " free frames"
        ));
    }

    #[test]
    fn the_expected_list_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for line in EXPECTED_TEXT {
            assert!(seen.insert(line), "duplicate expected line: {line}");
        }
    }
}
