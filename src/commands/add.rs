//! `grm add`: add a repo to the manifest and clone it.
//!
//! - Load manifest (`manifest::discover(ctx.manifest_path.as_deref())`).
//! - Parse `args.spec` with `spec::parse`; error if a repo with the same
//!   `spec::normalize_url` is already listed.
//! - Append `RepoEntry::Short(spec)` if no `--path/--name/--email/--branch`,
//!   else `RepoEntry::Full` (identity from `--name/--email`). Save.
//! - Unless `--no-clone`: clone to the resolved path (skip if it exists)
//!   and apply the merged identity's git config.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::AddArgs;
use crate::commands::Context;

/// Run `grm add`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: AddArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm add")
}
