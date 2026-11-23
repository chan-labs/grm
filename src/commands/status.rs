//! `grm status`: show drift between manifest and disk (alias: `diff`).
//!
//! - `state::inspect` the manifest and print each repo with its state
//!   (coloured with `owo-colors`), then extras.
//! - Exit code: `ExitCode::from(1)` if `Report::has_drift()`, else success.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::StatusArgs;
use crate::commands::Context;

/// Run `grm status`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: StatusArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm status")
}
