//! `grm sync`: make disk match the manifest.
//!
//! - `state::inspect` the manifest.
//! - `Missing` repos: `git::clone` (up to `--jobs` in parallel, e.g. with
//!   `std::thread::scope`), then apply identity config.
//! - `Drifted`: fix `Config` (set/unset) and `Remote` drift; just report
//!   `Dirty` / `AheadBehind`.
//! - `--prune`: then behave like `grm prune` (honouring `--yes`).
//! - `--dry-run`: print planned actions only. Exit 1 if any repo failed.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::SyncArgs;
use crate::commands::Context;

/// Run `grm sync`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: SyncArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm sync")
}
