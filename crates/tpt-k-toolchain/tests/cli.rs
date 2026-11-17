// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runs the built `tpt-k-build` binary, as a user would.

use std::process::Command;

fn build(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tpt-k-build"))
        .args(args)
        .output()
        .expect("tpt-k-build runs")
}

#[test]
fn plan_prints_one_command_per_crate_grouped_by_target() {
    let out = build(&[
        "plan",
        "--fixture",
        "tpt-k-userland-shell,tpt-k-kernel,tpt-k-boot",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "cargo build -p tpt-k-kernel --target x86_64-unknown-none\n\
         cargo build -p tpt-k-boot --target x86_64-unknown-uefi\n\
         cargo build -p tpt-k-userland-shell\n"
    );
}

#[test]
fn help_exits_zero_and_says_cargo_is_not_run() {
    let out = build(&["help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("never runs cargo"));
}

#[test]
fn unknown_layer_fails_with_a_clear_message() {
    let out = build(&["plan", "--fixture", "serde"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("`serde` matches no tpt-k layer prefix"));
}

#[test]
fn metadata_file_is_read() {
    let dir = std::env::temp_dir().join(format!("tpt-k-build-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("metadata.json");
    std::fs::write(&path, r#"{"packages":[{"name":"tpt-k-ipc"}]}"#).unwrap();
    let out = build(&["check", "--metadata", path.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "cargo check -p tpt-k-ipc\n"
    );
}
