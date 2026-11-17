//! Boot image assembly and the QEMU launch, for `tpt-k-build image --run` and
//! `tpt-k-build run`.
//!
//! This module plans the steps and builds the argument lists. It runs nothing.
//! The CLI executes the plan. The plan matches `scripts/build-kernel.sh` and
//! `scripts/qemu-smoke.sh`, so the two paths produce the same image.

use std::path::{Path, PathBuf};

use crate::doctor::Os;

/// The target of the kernel image. Matches `scripts/build-kernel.sh`.
pub const TARGET_KERNEL: &str = "x86_64-unknown-none";
/// The target of the UEFI loader.
pub const TARGET_UEFI: &str = "x86_64-unknown-uefi";
/// The linker script for the kernel, relative to the repo root.
pub const KERNEL_LINKER_SCRIPT: &str = "crates/tpt-k-kernel/linker.ld";
/// The directory under the repo's target directory where the image is staged.
pub const OUT_DIR: &str = "target/tpt-k";

/// One file in the EFI System Partition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EspFile {
    /// Path inside the ESP, with `/` separators.
    pub dest: &'static str,
    /// Where the file comes from, as built.
    pub src: PathBuf,
}

/// One cargo invocation: the program and its arguments, run from the repo root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildStep {
    pub what: &'static str,
    pub args: Vec<String>,
}

/// The whole plan for an image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePlan {
    /// The directory the ESP is staged in, under the repo root.
    pub out: PathBuf,
    /// The builds, in order.
    pub builds: Vec<BuildStep>,
    /// The files that make up the ESP.
    pub esp: Vec<EspFile>,
    /// Where the kernel ELF is after the build.
    pub kernel_elf: PathBuf,
    /// Where the loader is after the build.
    pub loader_efi: PathBuf,
}

/// Plans the image for the repo at `root`. Nothing is built or copied.
#[must_use]
pub fn plan_image(root: &Path) -> ImagePlan {
    let target = root.join("target");
    let kernel_elf = target
        .join(TARGET_KERNEL)
        .join("debug")
        .join("tpt-k-kernel");
    let loader_efi = target
        .join(TARGET_UEFI)
        .join("debug")
        .join("tpt-k-boot.efi");
    let out = root.join(OUT_DIR);
    let esp = vec![
        EspFile {
            dest: "EFI/BOOT/BOOTX64.EFI",
            src: loader_efi.clone(),
        },
        EspFile {
            dest: "TPTK/KERNEL.ELF",
            src: kernel_elf.clone(),
        },
    ];
    ImagePlan {
        out,
        builds: vec![
            BuildStep {
                what: "kernel",
                args: vec![
                    "rustc".into(),
                    "-p".into(),
                    "tpt-k-kernel".into(),
                    "--bin".into(),
                    "tpt-k-kernel".into(),
                    "--target".into(),
                    TARGET_KERNEL.into(),
                    "--".into(),
                    "-C".into(),
                    "relocation-model=static".into(),
                    "-C".into(),
                    format!("link-arg=-T{KERNEL_LINKER_SCRIPT}"),
                ],
            },
            BuildStep {
                what: "loader",
                args: vec![
                    "build".into(),
                    "-p".into(),
                    "tpt-k-boot".into(),
                    "--target".into(),
                    TARGET_UEFI.into(),
                ],
            },
        ],
        esp,
        kernel_elf,
        loader_efi,
    }
}

/// Where the ESP's files are copied: `<out>/esp/<dest>`.
#[must_use]
pub fn esp_dir(plan: &ImagePlan) -> PathBuf {
    plan.out.join("esp")
}

/// Where the ESP's destination path lands on disk.
#[must_use]
pub fn esp_target(plan: &ImagePlan, dest: &str) -> PathBuf {
    dest.split('/')
        .fold(esp_dir(plan), |path, part| path.join(part))
}

/// How QEMU's serial port is connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Serial {
    /// Standard input and output of the `run` command.
    Stdio,
    /// A file, for the smoke test and for replay.
    File(PathBuf),
}

/// Settings for one QEMU run. Paths are as the host sees them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QemuRun {
    pub qemu: PathBuf,
    /// Read-only OVMF code image.
    pub ovmf_code: PathBuf,
    /// OVMF vars image, a writable copy for this run.
    pub ovmf_vars: PathBuf,
    /// The staged ESP directory.
    pub esp: PathBuf,
    pub serial: Serial,
    /// Run on QEMU's instruction counter instead of host time, so that two runs of
    /// the same image give the same serial transcript. Used for replay.
    pub icount: bool,
}

/// What one QEMU run produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QemuOutcome {
    /// QEMU's exit status. The kernel's debug exit makes it `(value << 1) | 1`.
    pub status: Option<i32>,
    /// Everything QEMU wrote to standard output, with `\r` removed.
    pub transcript: String,
}

/// Starts QEMU with its serial port on standard output, and waits for it.
///
/// The transcript comes from the pipe, not from a file. Writing the serial port to
/// a file dropped kernel output on this host, so a transcript read from a file was
/// incomplete. The pipe carries every byte the kernel sends. Fails after `timeout`,
/// when QEMU is killed. `launch.serial` is ignored: the transcript is always read
/// from stdout, so `Serial::Stdio` is what runs.
///
/// # Errors
///
/// Returns a message if QEMU cannot start, or does not finish in time.
pub fn capture(launch: &QemuRun, timeout: std::time::Duration) -> Result<QemuOutcome, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut args = qemu_args(launch);
    // The serial port goes to stdout, whatever `launch.serial` says.
    if let Some(at) = args.iter().position(|a| a == "-serial") {
        args[at + 1] = "stdio".to_string();
    }
    let mut child = Command::new(&launch.qemu)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start QEMU: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no QEMU stdout")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status.code();
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("QEMU did not finish within {}s", timeout.as_secs()));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let bytes = reader.join().unwrap_or_default();
    Ok(QemuOutcome {
        status,
        transcript: String::from_utf8_lossy(&bytes).replace('\r', ""),
    })
}

/// Masks the lines whose numbers come from host timing even on the instruction
/// counter. The APIC timer is calibrated against the PIT, which runs on host time, so
/// the count printed by the kernel changes from run to run by a few units. The line
/// stays in the trace, with the number replaced by `<calibrated>`.
#[must_use]
pub fn normalize_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix("apic: timer ") {
        if let Some((_, tail)) = rest.split_once(' ') {
            return format!("apic: timer <calibrated> {tail}");
        }
    }
    line.to_string()
}

/// The first line where `actual` differs from `golden`, described for a person, or
/// `None` if they are the same. Lines are compared after [`normalize_line`]. A
/// transcript that is only longer or shorter also differs, at the first line one of
/// them does not have.
#[must_use]
pub fn first_difference(golden: &str, actual: &str) -> Option<String> {
    let mut g = golden.lines();
    let mut a = actual.lines();
    let mut line = 1;
    loop {
        match (g.next(), a.next()) {
            (None, None) => return None,
            (Some(x), Some(y)) if normalize_line(x) == normalize_line(y) => line += 1,
            (x, y) => {
                return Some(format!(
                    "line {line}: golden has {:?}, this run has {:?}",
                    x.unwrap_or("<end of trace>"),
                    y.unwrap_or("<end of transcript>")
                ))
            }
        }
    }
}

/// The QEMU command line for `run`. Windows paths are handled by the caller.
#[must_use]
pub fn qemu_args(run: &QemuRun) -> Vec<String> {
    let serial = match &run.serial {
        Serial::Stdio => "stdio".to_string(),
        Serial::File(path) => format!("file:{}", path.display()),
    };
    let mut args: Vec<String> = vec![
        "-machine".into(),
        "q35".into(),
        "-m".into(),
        "256M".into(),
        "-no-reboot".into(),
        "-serial".into(),
        serial,
        "-drive".into(),
        format!(
            "if=pflash,format=raw,unit=0,readonly=on,file={}",
            run.ovmf_code.display()
        ),
        "-drive".into(),
        format!(
            "if=pflash,format=raw,unit=1,file={}",
            run.ovmf_vars.display()
        ),
        "-drive".into(),
        format!("file=fat:rw:{},format=raw,media=disk", run.esp.display()),
        "-device".into(),
        "isa-debug-exit,iobase=0xf4,iosize=0x04".into(),
    ];
    if run.icount {
        // The RTC follows the virtual clock too. Otherwise its interrupts follow
        // host time, and the order in which the demo threads finish changes.
        args.extend([
            "-icount".into(),
            "shift=auto,sleep=off".into(),
            "-rtc".into(),
            "clock=vm".into(),
        ]);
    }
    args
}

/// Where to look for OVMF code images, first match wins. Environment variable
/// `OVMF_CODE` overrides these.
#[must_use]
pub fn ovmf_candidates(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &[r"C:\Program Files\qemu\share\edk2-x86_64-code.fd"],
        Os::Linux => &[
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        ],
    }
}

/// Where to look for OVMF vars templates. Environment variable `OVMF_VARS`
/// overrides these.
#[must_use]
pub fn ovmf_vars_candidates(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &[r"C:\Program Files\qemu\share\edk2-i386-vars.fd"],
        Os::Linux => &[
            "/usr/share/OVMF/OVMF_VARS_4M.fd",
            "/usr/share/OVMF/OVMF_VARS.fd",
            "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
            "/usr/share/edk2-ovmf/x64/OVMF_VARS.fd",
        ],
    }
}

/// The default install folder of QEMU on Windows.
pub const WINDOWS_QEMU: &str = r"C:\Program Files\qemu\qemu-system-x86_64.exe";

#[cfg(test)]
mod replay_tests {
    use super::*;

    #[test]
    fn only_the_calibrated_timer_count_is_masked() {
        assert_eq!(
            normalize_line("apic: timer 624973 counts per 10 ms"),
            "apic: timer <calibrated> counts per 10 ms"
        );
        assert_eq!(
            normalize_line("preempt: ticks=5 timer switches=3"),
            "preempt: ticks=5 timer switches=3"
        );
    }

    #[test]
    fn identical_transcripts_have_no_difference() {
        let text = "Hello tpt-k\napic: timer 1 counts per 10 ms\nscheduler: ok\n";
        let other = "Hello tpt-k\napic: timer 2 counts per 10 ms\nscheduler: ok\n";
        assert_eq!(first_difference(text, other), None);
    }

    #[test]
    fn the_first_changed_line_is_named_with_both_sides() {
        let golden = "a\nb\nc\n";
        let actual = "a\nX\nc\n";
        let message = first_difference(golden, actual).unwrap();
        assert!(message.starts_with("line 2:"), "{message}");
        assert!(
            message.contains("\"b\"") && message.contains("\"X\""),
            "{message}"
        );
    }

    #[test]
    fn a_shorter_or_longer_transcript_differs_at_the_extra_line() {
        assert!(first_difference("a\nb\n", "a\n")
            .unwrap()
            .contains("<end of transcript>"));
        assert!(first_difference("a\n", "a\nb\n")
            .unwrap()
            .contains("<end of trace>"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn kernel_build_matches_the_build_script() {
        let plan = plan_image(&root());
        let kernel = &plan.builds[0];
        assert_eq!(kernel.what, "kernel");
        assert!(kernel.args.contains(&TARGET_KERNEL.to_string()));
        assert!(kernel
            .args
            .contains(&"link-arg=-Tcrates/tpt-k-kernel/linker.ld".to_string()));
        assert!(kernel.args.contains(&"relocation-model=static".to_string()));
    }

    #[test]
    fn loader_is_built_for_uefi() {
        let plan = plan_image(&root());
        let loader = &plan.builds[1];
        assert_eq!(loader.what, "loader");
        assert_eq!(
            loader.args,
            ["build", "-p", "tpt-k-boot", "--target", TARGET_UEFI]
        );
        assert!(plan
            .loader_efi
            .ends_with("x86_64-unknown-uefi/debug/tpt-k-boot.efi"));
    }

    #[test]
    fn esp_layout_is_the_one_the_loader_reads() {
        let plan = plan_image(&root());
        let dests: Vec<&str> = plan.esp.iter().map(|f| f.dest).collect();
        assert_eq!(dests, ["EFI/BOOT/BOOTX64.EFI", "TPTK/KERNEL.ELF"]);
        assert_eq!(
            esp_target(&plan, "TPTK/KERNEL.ELF"),
            root()
                .join(OUT_DIR)
                .join("esp")
                .join("TPTK")
                .join("KERNEL.ELF")
        );
    }

    #[test]
    fn qemu_arguments_match_the_smoke_test_and_use_stdio_when_asked() {
        let run = QemuRun {
            qemu: PathBuf::from("qemu"),
            ovmf_code: PathBuf::from("code.fd"),
            ovmf_vars: PathBuf::from("vars.fd"),
            esp: PathBuf::from("esp"),
            serial: Serial::Stdio,
            icount: false,
        };
        let args = qemu_args(&run);
        assert!(args.windows(2).any(|w| w == ["-serial", "stdio"]));
        assert!(args.contains(&"isa-debug-exit,iobase=0xf4,iosize=0x04".to_string()));
        assert!(args.contains(&"file=fat:rw:esp,format=raw,media=disk".to_string()));

        let file = QemuRun {
            serial: Serial::File(PathBuf::from("serial.log")),
            ..run
        };
        assert!(qemu_args(&file).contains(&"file:serial.log".to_string()));
    }

    #[test]
    fn ovmf_search_lists_are_not_empty_on_each_os() {
        for os in [Os::Windows, Os::Linux] {
            assert!(!ovmf_candidates(os).is_empty());
            assert!(!ovmf_vars_candidates(os).is_empty());
        }
    }
}
