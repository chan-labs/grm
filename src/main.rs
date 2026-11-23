//! `grm` entry point: parse the command line and dispatch to a subcommand.
//!
//! Each subcommand lives in `src/commands/<name>.rs` and exposes
//! `pub fn run(args, &Context) -> anyhow::Result<ExitCode>`.

// RUST: `mod foo;` tells the compiler to include `src/foo.rs` (or
// `src/foo/mod.rs`) as a module. Modules are Rust's unit of code organisation.
mod cli;
mod commands;
mod git;
mod manifest;
mod spec;
mod state;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::commands::Context;

// RUST: `main` may return an `ExitCode` instead of `()`. Whatever it returns
// becomes the process exit status (`ExitCode::SUCCESS` = 0, `from(1)` = 1),
// which is how `grm status` can "fail" in scripts without printing an error.
fn main() -> ExitCode {
    // RUST: `Cli::parse()` comes from the `Parser` derive. On bad input it
    // prints usage and exits the process itself.
    let cli = Cli::parse();

    let ctx = Context {
        manifest_path: cli.file,
    };

    // RUST: `match` must handle every enum variant – the compiler checks this,
    // so adding a subcommand without dispatching it is a compile error.
    let result = match cli.command {
        Command::Init(args) => commands::init::run(args, &ctx),
        Command::Add(args) => commands::add::run(args, &ctx),
        Command::Remove(args) => commands::remove::run(args, &ctx),
        Command::Sync(args) => commands::sync::run(args, &ctx),
        Command::Status(args) => commands::status::run(args, &ctx),
        Command::Prune(args) => commands::prune::run(args, &ctx),
        Command::Import(args) => commands::import::run(args, &ctx),
    };

    // RUST: `Result<T, E>` is either `Ok(T)` or `Err(E)`. Here we turn an
    // error into a message on stderr and exit code 1.
    match result {
        Ok(code) => code,
        Err(err) => {
            // RUST: `{:#}` on an anyhow error prints the whole context chain
            // on one line, e.g. "loading manifest: No such file".
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
