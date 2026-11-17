//! `tpt-k-build doctor`: checks that the machine can build and boot tpt-k, and
//! prints the exact fix for each problem.
//!
//! This module holds the logic only: what each check needs, how a probe result
//! reads, and how the report looks for Windows or Linux. The probes themselves
//! (running `rustc`, looking for QEMU, reading git config) live in the CLI.

use std::fmt::Write as _;

/// The Rust version the workspace pins in `rust-toolchain.toml`.
pub const REQUIRED_RUST: &str = "1.95.0";

/// The bare-metal and UEFI targets the kernel and the loader are built for.
pub const REQUIRED_TARGETS: [&str; 2] = ["x86_64-unknown-none", "x86_64-unknown-uefi"];

/// The sibling TPT repositories the crates use by path. `cargo xtask fetch-deps`
/// clones and updates exactly these.
pub const SIBLING_REPOS: [&str; 10] = [
    "tpt-crypto",
    "tpt-crypto-rustls",
    "tpt-primitives",
    "tpt-simd",
    "tpt-fabric",
    "tpt-mcpbox",
    "tpt-concord",
    "tpt-filesystem",
    "tpt-cargo-vitals",
    "tpt-semadiff",
];

/// The git setting that enables the pre-push hook.
pub const HOOKS_PATH: &str = ".githooks";

/// Which platform's fix commands to print.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
}

impl Os {
    /// The platform this binary was built for.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// One thing the doctor checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    RustToolchain,
    Targets,
    Qemu,
    Ovmf,
    SiblingRepos,
    HooksPath,
}

impl Check {
    /// Every check, in the order the report prints them.
    pub const ALL: [Check; 6] = [
        Check::RustToolchain,
        Check::Targets,
        Check::Qemu,
        Check::Ovmf,
        Check::SiblingRepos,
        Check::HooksPath,
    ];

    /// The short name shown in the report.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Check::RustToolchain => "rust toolchain",
            Check::Targets => "rust targets",
            Check::Qemu => "qemu",
            Check::Ovmf => "ovmf firmware",
            Check::SiblingRepos => "sibling repos",
            Check::HooksPath => "git hooks path",
        }
    }

    /// Whether a failure stops the build or boot (`true`), or is only advice.
    #[must_use]
    pub fn is_required(self) -> bool {
        !matches!(self, Check::HooksPath)
    }
}

/// What a probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The check passed. The text says what was found, for example a version.
    Ok(String),
    /// Something is there, but not the right one. The text says what it is.
    Wrong(String),
    /// Nothing was found. The text says what was looked for.
    Missing(String),
}

/// One check and what its probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    pub probe: Probe,
}

/// Pulls the version out of `rustc --version` output, such as
/// `rustc 1.95.0 (f2d3ce0bd 2026-03-21)`.
#[must_use]
pub fn parse_rustc_version(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    if words.next()? != "rustc" {
        return None;
    }
    let version = words.next()?;
    let valid =
        version.split('.').count() >= 2 && version.chars().all(|c| c.is_ascii_digit() || c == '.');
    valid.then(|| version.to_string())
}

/// The required targets that the output of `rustup target list --installed`
/// does not name, in the order they are required.
#[must_use]
pub fn missing_targets(installed: &str) -> Vec<&'static str> {
    let listed: Vec<&str> = installed.split_whitespace().collect();
    REQUIRED_TARGETS
        .iter()
        .copied()
        .filter(|t| !listed.contains(t))
        .collect()
}

/// The fix commands for `check` on `os`. Each line is one command or one step.
#[must_use]
pub fn fix(check: Check, os: Os) -> Vec<String> {
    let lines: &[&str] = match (check, os) {
        (Check::RustToolchain, _) => &[
            "rustup toolchain install 1.95.0",
            "(the repo's rust-toolchain.toml then selects it; `rustup show` installs the rest)",
        ],
        (Check::Targets, _) => &["rustup target add x86_64-unknown-none x86_64-unknown-uefi"],
        (Check::Qemu, Os::Windows) => &[
            "winget install --id SoftwareFreedomConservancy.QEMU -e",
            "then add C:\\Program Files\\qemu to PATH, or set QEMU to qemu-system-x86_64.exe",
        ],
        (Check::Qemu, Os::Linux) => &["sudo apt install qemu-system-x86"],
        (Check::Ovmf, Os::Windows) => &[
            "install QEMU: its share\\edk2-x86_64-code.fd is the firmware",
            "or set OVMF_CODE to the path of an OVMF_CODE .fd file",
        ],
        (Check::Ovmf, Os::Linux) => &["sudo apt install ovmf"],
        (Check::SiblingRepos, Os::Windows) => &["bash scripts/fetch-deps.sh   (in Git Bash)"],
        (Check::SiblingRepos, Os::Linux) => &["scripts/fetch-deps.sh"],
        (Check::HooksPath, _) => &["git config core.hooksPath .githooks"],
    };
    lines.iter().map(|s| (*s).to_string()).collect()
}

/// Whether a finding fails the doctor. Required checks that are not `Ok` fail.
#[must_use]
pub fn fails(finding: &Finding) -> bool {
    finding.check.is_required() && !matches!(finding.probe, Probe::Ok(_))
}

/// Renders the report and says whether every required check passed.
///
/// Each line starts with `ok`, `warn` or `FAIL`. Every problem is followed by
/// its fix. The last line is the summary.
#[must_use]
pub fn render(findings: &[Finding], os: Os) -> (String, bool) {
    let mut text = String::from("tpt-k doctor\n");
    let mut failures = 0;
    for check in Check::ALL {
        let Some(finding) = findings.iter().find(|f| f.check == check) else {
            // No probe result for this check: report it as missing.
            let _ = writeln!(text, "FAIL {}: not probed", check.name());
            failures += 1;
            continue;
        };
        match &finding.probe {
            Probe::Ok(detail) => {
                let _ = writeln!(text, "ok   {}: {detail}", check.name());
            }
            Probe::Wrong(detail) | Probe::Missing(detail) => {
                let tag = if check.is_required() { "FAIL" } else { "warn" };
                let _ = writeln!(text, "{tag} {}: {detail}", check.name());
                for line in fix(check, os) {
                    let _ = writeln!(text, "       fix: {line}");
                }
                if fails(finding) {
                    failures += 1;
                }
            }
        }
    }
    let ok = failures == 0;
    if ok {
        text.push_str("doctor: all required checks passed\n");
    } else {
        let _ = writeln!(text, "doctor: {failures} required check(s) failed");
    }
    (text, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_ok() -> Vec<Finding> {
        Check::ALL
            .iter()
            .map(|&check| Finding {
                check,
                probe: Probe::Ok("found".to_string()),
            })
            .collect()
    }

    #[test]
    fn parses_rustc_version_lines() {
        assert_eq!(
            parse_rustc_version("rustc 1.95.0 (f2d3ce0bd 2026-03-21)\n"),
            Some("1.95.0".to_string())
        );
        assert_eq!(parse_rustc_version("cargo 1.95.0"), None);
        assert_eq!(parse_rustc_version(""), None);
        assert_eq!(parse_rustc_version("rustc nightly-x"), None);
    }

    #[test]
    fn missing_targets_are_reported_in_required_order() {
        assert!(missing_targets("x86_64-unknown-uefi\nx86_64-unknown-none\n").is_empty());
        assert_eq!(
            missing_targets("aarch64-unknown-none\n"),
            vec!["x86_64-unknown-none", "x86_64-unknown-uefi"]
        );
        assert_eq!(
            missing_targets("x86_64-unknown-none"),
            vec!["x86_64-unknown-uefi"]
        );
    }

    #[test]
    fn all_ok_passes_with_no_fixes() {
        let (text, ok) = render(&all_ok(), Os::Linux);
        assert!(ok);
        assert!(!text.contains("fix:"));
        assert!(text.ends_with("doctor: all required checks passed\n"));
    }

    #[test]
    fn missing_required_check_fails_and_prints_its_fix() {
        let mut findings = all_ok();
        findings[2].probe = Probe::Missing("qemu-system-x86_64 not on PATH".to_string());
        let (text, ok) = render(&findings, Os::Windows);
        assert!(!ok);
        assert!(text.contains("FAIL qemu: qemu-system-x86_64 not on PATH"));
        assert!(text.contains("winget install --id SoftwareFreedomConservancy.QEMU"));
        assert!(text.ends_with("doctor: 1 required check(s) failed\n"));
    }

    #[test]
    fn linux_fixes_use_apt_and_scripts() {
        assert_eq!(
            fix(Check::Qemu, Os::Linux),
            vec!["sudo apt install qemu-system-x86"]
        );
        assert_eq!(
            fix(Check::SiblingRepos, Os::Linux),
            vec!["scripts/fetch-deps.sh"]
        );
    }

    #[test]
    fn missing_hooks_path_warns_but_does_not_fail() {
        let mut findings = all_ok();
        findings[5].probe = Probe::Missing("core.hooksPath is not set".to_string());
        let (text, ok) = render(&findings, Os::Linux);
        assert!(ok);
        assert!(text.contains("warn git hooks path"));
        assert!(text.contains("git config core.hooksPath .githooks"));
    }

    #[test]
    fn wrong_rust_version_fails() {
        let mut findings = all_ok();
        findings[0].probe = Probe::Wrong("rustc 1.90.0 (pinned is 1.95.0)".to_string());
        let (_, ok) = render(&findings, Os::Linux);
        assert!(!ok);
    }

    #[test]
    fn absent_probe_result_counts_as_a_failure() {
        let findings: Vec<Finding> = all_ok().into_iter().take(3).collect();
        let (text, ok) = render(&findings, Os::Linux);
        assert!(!ok);
        assert!(text.contains("FAIL ovmf firmware: not probed"));
    }
}
