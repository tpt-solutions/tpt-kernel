// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runs the built shell binary with a script on standard input.

use std::io::Write;
use std::process::{Command, Stdio};

fn run_script(script: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tpt-k-userland"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn shell binary");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write script");
    let output = child.wait_with_output().expect("wait for shell");
    (
        output.status.code().expect("exit code"),
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        String::from_utf8(output.stderr).expect("utf-8 stderr"),
    )
}

#[test]
fn script_runs_builtins_and_exits_with_its_code() {
    let (code, stdout, stderr) =
        run_script("echo hello\nfalse || echo recovered\nexit 5\necho no\n");
    assert_eq!(code, 5);
    assert_eq!(stdout, "> hello\n> recovered\n> ");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
}

#[test]
fn end_of_input_returns_last_status() {
    let (code, _, _) = run_script("true\nls\n");
    assert_eq!(code, 127);
}

#[test]
fn unknown_command_message_goes_to_stderr() {
    let (_, stdout, stderr) = run_script("frobnicate\nexit 0\n");
    assert_eq!(stderr, "tpt-k: frobnicate: not supported yet\n");
    assert_eq!(stdout, "> > ");
}
