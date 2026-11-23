//! `grm remove`: remove a repo from the manifest.
//!
//! - Find the entry whose `spec::normalize_url` matches `args.spec`'s;
//!   error if none. Remove it and save.
//! - With `--delete`: refuse if the checkout is dirty / has unpushed commits /
//!   has stashes unless `--force`; ask `commands::confirm` unless `--yes`;
//!   then delete the directory.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::RemoveArgs;
use crate::commands::Context;

/// Run `grm remove`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: RemoveArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm remove")
}
