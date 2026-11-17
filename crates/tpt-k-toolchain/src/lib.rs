// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k build planner.
//!
//! The planner turns a list of workspace crates into cargo commands. Each
//! crate is placed in a layer by its name prefix, and each layer has a target:
//! kernel-side crates build for `x86_64-unknown-none`, loader crates for
//! `x86_64-unknown-uefi`, and everything else for the host.
//!
//! This version only plans. It never runs cargo, and it reads workspace
//! metadata from text that the caller provides.
//!
//! The [`assurance`] module is the other half of the CLI: it checks the
//! assurance graph that links claims to the tests, exhaustive checks and fuzz
//! targets that back them.

pub mod assurance;
pub mod caps;
pub mod doctor;
pub mod runner;

use serde_json::Value;
use std::fmt;

/// Target triple for kernel-side crates.
pub const TARGET_KERNEL: &str = "x86_64-unknown-none";
/// Target triple for the UEFI loader.
pub const TARGET_UEFI: &str = "x86_64-unknown-uefi";

/// A layer of the tpt-k tree, named by its crate prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// `tpt-k-kernel*`: the microkernel, built for `x86_64-unknown-none`.
    Kernel,
    /// `tpt-k-boot*`: the UEFI loader, built for `x86_64-unknown-uefi`.
    Boot,
    /// `tpt-k-root*`: the root server, built for the host in this version.
    Root,
    /// `tpt-k-hal*`: hardware description parsers, built for the host.
    Hal,
    /// `tpt-k-ipc*`: user-space IPC libraries, built for the host.
    Ipc,
    /// `tpt-k-drivers*`: driver manager and drivers, built for the host.
    Drivers,
    /// `tpt-k-userland*`: shell, libc, network and the filesystem bridge.
    Userland,
    /// `tpt-k-toolchain*`: this build tool and image assembly.
    Toolchain,
}

/// Prefix-to-layer table. A prefix matches a crate name exactly or when the
/// name continues with `-`, so `tpt-k-hal` matches `tpt-k-hal-acpi` but not
/// `tpt-k-halt`.
const PREFIXES: &[(&str, Layer)] = &[
    ("tpt-k-kernel", Layer::Kernel),
    ("tpt-k-boot", Layer::Boot),
    ("tpt-k-root", Layer::Root),
    ("tpt-k-hal", Layer::Hal),
    ("tpt-k-ipc", Layer::Ipc),
    ("tpt-k-drivers", Layer::Drivers),
    ("tpt-k-userland", Layer::Userland),
    ("tpt-k-toolchain", Layer::Toolchain),
];

impl Layer {
    /// The layer a crate name belongs to, or an error if no prefix matches.
    pub fn of(crate_name: &str) -> Result<Self, PlanError> {
        PREFIXES
            .iter()
            .find(|(prefix, _)| {
                crate_name == *prefix
                    || crate_name
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('-'))
            })
            .map(|(_, layer)| *layer)
            .ok_or_else(|| PlanError::UnknownLayer(crate_name.to_string()))
    }

    /// The build target for the layer.
    #[must_use]
    pub const fn target(self) -> Target {
        match self {
            Self::Kernel => Target::Kernel,
            Self::Boot => Target::Uefi,
            _ => Target::Host,
        }
    }
}

/// A build target. The declaration order is the order of groups in a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// `x86_64-unknown-none`.
    Kernel,
    /// `x86_64-unknown-uefi`.
    Uefi,
    /// The host, passed to cargo with no `--target` flag.
    Host,
}

impl Target {
    /// The `--target` triple, or `None` for the host.
    #[must_use]
    pub const fn triple(self) -> Option<&'static str> {
        match self {
            Self::Kernel => Some(TARGET_KERNEL),
            Self::Uefi => Some(TARGET_UEFI),
            Self::Host => None,
        }
    }
}

/// Which cargo subcommand a plan runs for each crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subcommand {
    /// `cargo build`.
    Build,
    /// `cargo check`.
    Check,
}

impl Subcommand {
    fn word(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Check => "check",
        }
    }
}

/// A workspace crate, with its layer resolved from its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateInfo {
    /// The package name.
    pub name: String,
    /// The layer the name belongs to.
    pub layer: Layer,
}

impl CrateInfo {
    /// A crate with its layer taken from its name.
    pub fn new(name: &str) -> Result<Self, PlanError> {
        Ok(Self {
            name: name.to_string(),
            layer: Layer::of(name)?,
        })
    }

    /// The target this crate builds for.
    #[must_use]
    pub const fn target(&self) -> Target {
        self.layer.target()
    }
}

/// One cargo command in a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The crate the command is for.
    pub crate_name: String,
    /// The target the command builds for.
    pub target: Target,
    /// The subcommand the command runs.
    pub subcommand: Subcommand,
}

impl Step {
    /// The cargo arguments for this step, starting with `cargo`.
    #[must_use]
    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            "cargo".to_string(),
            self.subcommand.word().to_string(),
            "-p".to_string(),
            self.crate_name.clone(),
        ];
        if let Some(triple) = self.target.triple() {
            args.push("--target".to_string());
            args.push(triple.to_string());
        }
        args
    }

    /// The command as one line of text.
    #[must_use]
    pub fn command_line(&self) -> String {
        self.args().join(" ")
    }
}

/// Steps grouped by target, in a fixed order: kernel, UEFI, then host. Within
/// a group the steps are sorted by crate name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The steps, in the order they should run.
    pub steps: Vec<Step>,
}

impl Plan {
    /// Every step, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter()
    }

    /// The plan as text, one cargo command per line.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            out.push_str(&step.command_line());
            out.push('\n');
        }
        out
    }
}

/// Builds the plan for `crates`. Duplicate names are planned once.
#[must_use]
pub fn plan(crates: &[CrateInfo], subcommand: Subcommand) -> Plan {
    let mut sorted: Vec<&CrateInfo> = crates.iter().collect();
    sorted.sort_by(|a, b| (a.target(), &a.name).cmp(&(b.target(), &b.name)));
    sorted.dedup_by(|a, b| a.name == b.name);
    let steps = sorted
        .into_iter()
        .map(|c| Step {
            crate_name: c.name.clone(),
            target: c.target(),
            subcommand,
        })
        .collect();
    Plan { steps }
}

/// The steps that assemble the boot image, as text lines. Nothing here is run:
/// `tpt-k-build image --run` runs them.
///
/// The steps are the ones in [`runner::plan_image`]. The kernel and the loader
/// are built, and the EFI System Partition is staged in `target/tpt-k/esp`. The
/// root server is not part of the image yet, and the FAT32 writer in
/// `tpt-k-toolchain-iso` is not used, since the staged directory is read by QEMU's
/// FAT driver.
#[must_use]
pub fn image_steps() -> Vec<String> {
    vec![
        "1. cargo rustc -p tpt-k-kernel --bin tpt-k-kernel --target x86_64-unknown-none (kernel, with its linker script)".to_string(),
        format!("2. cargo build -p tpt-k-boot --target {TARGET_UEFI} (loader)"),
        "3. stage target/tpt-k/esp: EFI/BOOT/BOOTX64.EFI, TPTK/KERNEL.ELF".to_string(),
    ]
}

/// Reads the package names from the output of
/// `cargo metadata --no-deps --format-version 1`, and resolves each layer.
pub fn crates_from_metadata(json: &str) -> Result<Vec<CrateInfo>, PlanError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| PlanError::Metadata(e.to_string()))?;
    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| PlanError::Metadata("no `packages` array".to_string()))?;
    packages
        .iter()
        .map(|pkg| {
            let name = pkg
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| PlanError::Metadata("package without a name".to_string()))?;
            CrateInfo::new(name)
        })
        .collect()
}

/// Why a plan could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// A crate name matches none of the known layer prefixes.
    UnknownLayer(String),
    /// The metadata text is not the expected JSON.
    Metadata(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLayer(name) => write!(
                f,
                "crate `{name}` matches no tpt-k layer prefix (kernel, boot, root, hal, ipc, drivers, userland, toolchain)"
            ),
            Self::Metadata(why) => write!(f, "cannot read cargo metadata: {why}"),
        }
    }
}

impl std::error::Error for PlanError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str) -> CrateInfo {
        CrateInfo::new(name).unwrap()
    }

    #[test]
    fn prefixes_resolve_to_layers() {
        assert_eq!(Layer::of("tpt-k-kernel-mem").unwrap(), Layer::Kernel);
        assert_eq!(Layer::of("tpt-k-kernel").unwrap(), Layer::Kernel);
        assert_eq!(Layer::of("tpt-k-boot-efi").unwrap(), Layer::Boot);
        assert_eq!(Layer::of("tpt-k-hal-acpi").unwrap(), Layer::Hal);
        assert_eq!(Layer::of("tpt-k-drivers-nvme").unwrap(), Layer::Drivers);
        assert_eq!(Layer::of("tpt-k-userland-net").unwrap(), Layer::Userland);
        assert_eq!(Layer::of("tpt-k-toolchain-iso").unwrap(), Layer::Toolchain);
    }

    #[test]
    fn prefix_must_end_at_a_dash() {
        // `tpt-k-halt` is not a `tpt-k-hal` crate.
        assert_eq!(
            Layer::of("tpt-k-halt"),
            Err(PlanError::UnknownLayer("tpt-k-halt".to_string()))
        );
    }

    #[test]
    fn unknown_prefix_is_rejected_with_the_name() {
        let err = CrateInfo::new("tpt-other").unwrap_err();
        assert_eq!(err, PlanError::UnknownLayer("tpt-other".to_string()));
        assert!(err.to_string().contains("`tpt-other`"));
    }

    #[test]
    fn layers_map_to_targets() {
        assert_eq!(Layer::Kernel.target(), Target::Kernel);
        assert_eq!(Layer::Boot.target(), Target::Uefi);
        for layer in [
            Layer::Root,
            Layer::Hal,
            Layer::Ipc,
            Layer::Drivers,
            Layer::Userland,
            Layer::Toolchain,
        ] {
            assert_eq!(layer.target(), Target::Host);
        }
        assert_eq!(Target::Host.triple(), None);
        assert_eq!(Target::Kernel.triple(), Some("x86_64-unknown-none"));
        assert_eq!(Target::Uefi.triple(), Some("x86_64-unknown-uefi"));
    }

    #[test]
    fn plan_groups_by_target_then_name() {
        let crates = [
            info("tpt-k-userland-shell"),
            info("tpt-k-kernel-mem"),
            info("tpt-k-boot"),
            info("tpt-k-hal-acpi"),
            info("tpt-k-kernel"),
        ];
        let p = plan(&crates, Subcommand::Build);
        let lines: Vec<String> = p.iter().map(Step::command_line).collect();
        assert_eq!(
            lines,
            [
                "cargo build -p tpt-k-kernel --target x86_64-unknown-none",
                "cargo build -p tpt-k-kernel-mem --target x86_64-unknown-none",
                "cargo build -p tpt-k-boot --target x86_64-unknown-uefi",
                "cargo build -p tpt-k-hal-acpi",
                "cargo build -p tpt-k-userland-shell",
            ]
        );
    }

    #[test]
    fn plan_is_deterministic_regardless_of_input_order() {
        let a = [info("tpt-k-hal"), info("tpt-k-ipc"), info("tpt-k-root")];
        let b = [info("tpt-k-root"), info("tpt-k-ipc"), info("tpt-k-hal")];
        assert_eq!(
            plan(&a, Subcommand::Check).render(),
            plan(&b, Subcommand::Check).render()
        );
    }

    #[test]
    fn duplicate_names_are_planned_once() {
        let crates = [info("tpt-k-ipc"), info("tpt-k-ipc")];
        assert_eq!(plan(&crates, Subcommand::Build).steps.len(), 1);
    }

    #[test]
    fn check_subcommand_uses_cargo_check() {
        let p = plan(&[info("tpt-k-kernel")], Subcommand::Check);
        assert_eq!(
            p.steps[0].args(),
            [
                "cargo",
                "check",
                "-p",
                "tpt-k-kernel",
                "--target",
                "x86_64-unknown-none"
            ]
        );
    }

    #[test]
    fn metadata_yields_crates_with_layers() {
        let json = r#"{"packages":[{"name":"tpt-k-boot"},{"name":"tpt-k-ipc-cap","version":"0.0.0"}],"workspace_members":[]}"#;
        let crates = crates_from_metadata(json).unwrap();
        assert_eq!(crates.len(), 2);
        assert_eq!(crates[0].layer, Layer::Boot);
        assert_eq!(crates[1].layer, Layer::Ipc);
    }

    #[test]
    fn metadata_errors_are_reported() {
        assert!(matches!(
            crates_from_metadata("not json"),
            Err(PlanError::Metadata(_))
        ));
        assert!(matches!(
            crates_from_metadata(r#"{"other":1}"#),
            Err(PlanError::Metadata(_))
        ));
        assert_eq!(
            crates_from_metadata(r#"{"packages":[{"name":"serde"}]}"#),
            Err(PlanError::UnknownLayer("serde".to_string()))
        );
    }

    #[test]
    fn image_steps_name_the_kernel_loader_and_esp() {
        let steps = image_steps();
        assert_eq!(steps.len(), 3);
        assert!(steps[0].contains("tpt-k-kernel --bin tpt-k-kernel --target x86_64-unknown-none"));
        assert!(steps[1].contains("tpt-k-boot --target x86_64-unknown-uefi"));
        assert!(steps[2].contains("BOOTX64.EFI"));
        assert!(steps[2].contains("KERNEL.ELF"));
    }
}
