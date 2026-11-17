// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k shell binary. Reads lines from standard input and runs them through
//! the REPL. Works with piped input, and uses no terminal control.

use std::io::{self, Write};

use tpt_k_userland::{run_reader, Repl, Sink};

/// A sink that writes to the process's standard streams.
struct Std;

impl Sink for Std {
    fn out(&mut self, text: &str) {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }

    fn err(&mut self, text: &str) {
        let _ = io::stderr().lock().write_all(text.as_bytes());
    }
}

fn main() {
    let mut repl = Repl::new();
    let stdin = io::stdin().lock();
    // The prompt is always written. There is no terminal detection, so piped
    // and interactive runs produce the same transcript.
    let code = match run_reader(&mut repl, stdin, &mut Std, true) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tpt-k: read error: {e}");
            1
        }
    };
    std::process::exit(code);
}
