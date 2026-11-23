//! Command-line interface definition.
//!
//! This module only *describes* the CLI (flags, subcommands, help text).
//! Parsing is done by `clap`; behaviour lives in `crate::commands`.

// RUST: `use` brings names into scope, like `import` in other languages.
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Manage git repositories declaratively from a `grm.yml` manifest.
// RUST: `#[derive(...)]` asks the compiler to auto-generate trait
// implementations. `Parser` (from clap) generates the argument parser from
// this struct's fields; `Debug` lets us print it with `{:?}`.
// The `///` doc comments become the `--help` text.
#[derive(Debug, Parser)]
#[command(name = "grm", version, about, long_about = None)]
pub struct Cli {
    /// Path to the manifest (defaults to discovering `grm.yml`).
    // RUST: `Option<T>` is either `Some(value)` or `None` – Rust has no null.
    // `global = true` lets the flag appear before or after the subcommand.
    #[arg(short = 'f', long = "file", env = "GRM_FILE", global = true)]
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// The available subcommands.
// RUST: an `enum` here is a "sum type": a value is exactly one of these
// variants, and each variant can carry its own data (here, an args struct).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new grm.yml manifest.
    Init(InitArgs),
    /// Add a repository to the manifest (and clone it).
    Add(AddArgs),
    /// Remove a repository from the manifest.
    Remove(RemoveArgs),
    /// Clone missing repos and apply configuration so reality matches the manifest.
    Sync(SyncArgs),
    /// Show how the repos on disk differ from the manifest.
    #[command(visible_alias = "diff")]
    Status(StatusArgs),
    /// Delete repos under the root that are not listed in the manifest.
    Prune(PruneArgs),
    /// Scan a directory for existing repos and add them to the manifest.
    Import(ImportArgs),
}

/// Arguments for `grm init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Root directory that repos are cloned under (stored in the manifest).
    #[arg(long, value_name = "DIR")]
    pub root: Option<String>,

    /// Overwrite an existing manifest.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `grm add`.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Repo to add: `owner/name`, `host/owner/name`, or a full git URL.
    pub spec: String,

    /// Custom checkout path (relative to root, or absolute / `~`).
    #[arg(long)]
    pub path: Option<String>,

    /// Git `user.name` to use for this repo.
    #[arg(long)]
    pub name: Option<String>,

    /// Git `user.email` to use for this repo.
    #[arg(long)]
    pub email: Option<String>,

    /// Branch to check out when cloning.
    #[arg(long)]
    pub branch: Option<String>,

    /// Only update the manifest; do not clone.
    #[arg(long)]
    pub no_clone: bool,
}

/// Arguments for `grm remove`.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Repo to remove (same formats as `add`).
    pub spec: String,

    /// Also delete the checkout from disk.
    #[arg(long)]
    pub delete: bool,

    /// Delete even if the repo has uncommitted/unpushed work.
    #[arg(long)]
    pub force: bool,

    /// Do not ask for confirmation.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Arguments for `grm sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Also prune repos not in the manifest.
    #[arg(long)]
    pub prune: bool,

    /// Print what would happen without changing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Number of parallel clone jobs.
    // RUST: `usize` is an unsigned integer the size of a pointer – the usual
    // type for counts and indexes.
    #[arg(short = 'j', long, default_value_t = 4)]
    pub jobs: usize,

    /// Do not ask for confirmation (e.g. when pruning).
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Arguments for `grm status` (none yet; kept for a uniform `run` signature).
#[derive(Debug, Args)]
pub struct StatusArgs {}

/// Arguments for `grm prune`.
#[derive(Debug, Args)]
pub struct PruneArgs {
    /// Do not ask for confirmation.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Delete even if a repo has uncommitted/unpushed work or stashes.
    #[arg(long)]
    pub force: bool,

    /// Print what would be deleted without deleting.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `grm import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Directory to scan (defaults to the manifest root).
    pub dir: Option<PathBuf>,

    /// How many directory levels deep to look for repos.
    #[arg(long, default_value_t = 2)]
    pub depth: usize,

    /// Print what would be added without changing the manifest.
    #[arg(long)]
    pub dry_run: bool,
}

// RUST: `#[cfg(test)]` means this module is only compiled for `cargo test`.
#[cfg(test)]
mod tests {
    // RUST: `super::*` imports everything from the parent module (this file).
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        // Catches clap misconfiguration (duplicate flags, etc.) at test time.
        Cli::command().debug_assert();
    }

    #[test]
    fn diff_is_alias_for_status() {
        let cli = Cli::try_parse_from(["grm", "diff"]).unwrap();
        assert!(matches!(cli.command, Command::Status(_)));
    }
}
