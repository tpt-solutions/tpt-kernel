// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k shell: command-line parser and job control.
//!
//! The crate is pure logic. It turns a typed line into a [`CommandLine`] and
//! keeps a [`JobTable`]. Running the programs is left to the process layer.

#![forbid(unsafe_code)]

pub mod jobs;
pub mod parse;

pub use jobs::{Job, JobError, JobState, JobTable};
pub use parse::{
    parse, AndOr, Chain, Command, CommandLine, ListItem, Op, ParseError, Pipeline, Redirect,
};
