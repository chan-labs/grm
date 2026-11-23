//! `grm prune`: delete repos under root that aren't in the manifest.
//!
//! - Use `state::inspect`'s `extras`.
//! - Skip (with a warning) repos that are dirty / have unpushed commits /
//!   have stashes unless `--force`. If those checks themselves fail, the repo
//!   is skipped too (when in doubt, don't delete).
//! - `--dry-run`: list only. Otherwise confirm via `commands::confirm`
//!   (once for the whole batch) unless `--yes`, then delete.
//! - Exit code 1 if any deletion failed. Skipped repos and answering "no" to
//!   the prompt are not failures.
//!
//! The core logic is [`prune_extras`], which `grm sync --prune` reuses.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result};

use crate::cli::PruneArgs;
use crate::commands::status::Painter;
use crate::commands::{Context, confirm};
use crate::git;
use crate::manifest;
use crate::state;

/// How [`prune_extras`] should behave (mirrors the `prune` flags).
#[derive(Debug, Clone, Copy, Default)]
pub struct PruneOptions {
    /// Don't ask for confirmation.
    pub yes: bool,
    /// Delete even repos with unsaved work.
    pub force: bool,
    /// Only print what would be deleted.
    pub dry_run: bool,
}

/// Run `grm prune`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: PruneArgs, ctx: &Context) -> Result<ExitCode> {
    let path = manifest::discover(ctx.manifest_path.as_deref())?;
    let m = manifest::load(&path)?;
    let report = state::inspect(&m).context("inspecting repos")?;

    let opts = PruneOptions {
        yes: args.yes,
        force: args.force,
        dry_run: args.dry_run,
    };
    Ok(if prune_extras(&report.extras, opts)? {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// Delete `extras` (repos not in the manifest), printing what happens.
///
/// Returns `Ok(true)` if every deletion that was attempted succeeded (or
/// nothing was attempted), `Ok(false)` if any failed. `Err` only for things
/// like failing to read the confirmation answer.
pub fn prune_extras(extras: &[PathBuf], opts: PruneOptions) -> Result<bool> {
    let p = Painter::new();
    if extras.is_empty() {
        println!("nothing to prune");
        return Ok(true);
    }

    // Split extras into "safe to delete" and "skipped, because ...".
    let mut to_delete = Vec::new();
    for path in extras {
        match unsafe_reason(path, opts.force) {
            Some(reason) => println!(
                "{} skip {}: {reason} (use --force to delete anyway)",
                p.warn("!"),
                path.display()
            ),
            None => to_delete.push(path),
        }
    }

    if to_delete.is_empty() {
        println!("nothing to prune");
        return Ok(true);
    }

    if opts.dry_run {
        for path in &to_delete {
            println!("{} would delete {}", p.dim("-"), path.display());
        }
        return Ok(true);
    }

    if !opts.yes {
        println!("repos to delete:");
        for path in &to_delete {
            println!("  {}", path.display());
        }
        let prompt = format!("Delete {} repo(s)?", to_delete.len());
        if !confirm(&prompt)? {
            println!("aborted; nothing deleted");
            return Ok(true);
        }
    }

    let mut all_ok = true;
    for path in to_delete {
        match std::fs::remove_dir_all(path) {
            Ok(()) => println!("{} deleted {}", p.ok("-"), path.display()),
            Err(e) => {
                all_ok = false;
                println!("{} failed to delete {}: {e}", p.err("✗"), path.display());
            }
        }
    }
    Ok(all_ok)
}

/// Why `path` must not be deleted, or `None` if it's safe (or `force`).
fn unsafe_reason(path: &Path, force: bool) -> Option<String> {
    if force {
        return None;
    }
    // RUST: a closure returning `Result` lets us use `?` for the three checks
    // and handle any error in one place below.
    let check = || -> Result<Vec<&str>> {
        let mut reasons = Vec::new();
        if git::is_dirty(path)? {
            reasons.push("uncommitted changes");
        }
        if git::has_stash(path)? {
            reasons.push("stashes");
        }
        if git::has_unpushed(path)? {
            reasons.push("unpushed commits");
        }
        Ok(reasons)
    };
    match check() {
        Ok(reasons) if reasons.is_empty() => None,
        Ok(reasons) => Some(format!("has {}", reasons.join(", "))),
        Err(e) => Some(format!("could not check for unsaved work ({e:#})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn run(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-c")
            .arg("init.defaultBranch=main")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init(dir: &Path) {
        fs::create_dir_all(dir).expect("mkdir");
        run(dir, &["init", "-q"]);
    }

    fn commit(dir: &Path) {
        run(
            dir,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "c",
            ],
        );
    }

    #[test]
    fn empty_repo_is_safe_but_dirty_and_unpushed_are_not() {
        let tmp = TempDir::new().expect("tempdir");
        let clean = tmp.path().join("clean");
        init(&clean);
        assert_eq!(unsafe_reason(&clean, false), None);

        let dirty = tmp.path().join("dirty");
        init(&dirty);
        fs::write(dirty.join("f"), "x").expect("write");
        assert_eq!(
            unsafe_reason(&dirty, false),
            Some("has uncommitted changes".to_string())
        );
        assert_eq!(unsafe_reason(&dirty, true), None);

        let unpushed = tmp.path().join("unpushed");
        init(&unpushed);
        commit(&unpushed);
        assert_eq!(
            unsafe_reason(&unpushed, false),
            Some("has unpushed commits".to_string())
        );
    }

    #[test]
    fn dry_run_and_yes() {
        let tmp = TempDir::new().expect("tempdir");
        let clean = tmp.path().join("clean");
        let dirty = tmp.path().join("dirty");
        init(&clean);
        init(&dirty);
        fs::write(dirty.join("f"), "x").expect("write");
        let extras = vec![clean.clone(), dirty.clone()];

        let dry = PruneOptions {
            dry_run: true,
            yes: true,
            ..Default::default()
        };
        assert!(prune_extras(&extras, dry).expect("prune"));
        assert!(clean.exists() && dirty.exists());

        let yes = PruneOptions {
            yes: true,
            ..Default::default()
        };
        assert!(prune_extras(&extras, yes).expect("prune"));
        assert!(!clean.exists(), "clean extra deleted");
        assert!(dirty.exists(), "dirty extra kept without --force");
    }
}
