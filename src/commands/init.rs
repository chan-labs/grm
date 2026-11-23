//! `grm init`: create a new manifest.
//!
//! - Path: `ctx.manifest_path` if set, else `./grm.yml` (do not use
//!   `manifest::discover`, which fails when no manifest exists yet).
//! - Refuse to overwrite an existing file unless `--force`.
//! - `root` = `--root` or `~/src`; defaults are `Defaults::default()`;
//!   `repos` empty. Write with `manifest::save` and print the path.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::InitArgs;
use crate::commands::Context;

/// Run `grm init`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: InitArgs, ctx: &Context) -> Result<ExitCode> {
    todo!("grm init")
}
