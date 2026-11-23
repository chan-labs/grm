//! `grm prune`: delete repos under root that aren't in the manifest.
//!
//! - Use `state::inspect`'s `extras`.
//! - Skip (with a warning) repos that are dirty / have unpushed commits /
//!   have stashes unless `--force`.
//! - `--dry-run`: list only. Otherwise confirm via `commands::confirm`
//!   unless `--yes`, then delete.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::PruneArgs;
use crate::commands::Context;

/// Run `grm prune`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: PruneArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm prune")
}
