//! `grm import`: add existing checkouts to the manifest.
//!
//! - Scan `args.dir` (default: manifest root) with
//!   `git::find_repos(dir, --depth)`.
//! - For each repo with an `origin`: skip if its normalized URL is already in
//!   the manifest; else add `RepoEntry::Short` if it sits at the default path,
//!   otherwise `RepoEntry::Full` with `path`.
//! - `--dry-run`: print only. Otherwise save and print a summary.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::ImportArgs;
use crate::commands::Context;

/// Run `grm import`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: ImportArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm import")
}
