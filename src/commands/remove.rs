//! `grm remove`: remove a repo from the manifest.
//!
//! - Find the entry to remove: parse `args.spec` with `spec::parse` and match
//!   by `spec::normalize_url`; if that finds nothing, fall back to matching
//!   the repo name (e.g. `cargo`) or the entry text exactly as written.
//!   Error if nothing matches or the match is ambiguous.
//! - With `--delete`: refuse if the checkout is dirty / has unpushed commits /
//!   has stashes unless `--force`; ask `commands::confirm` unless `--yes`.
//!   These checks run *before* the manifest is changed, so a refusal leaves
//!   everything as it was.
//! - Remove the entry and save; then (with `--delete`) delete the directory.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};

use crate::cli::RemoveArgs;
use crate::commands::{self, Context};
use crate::git;
use crate::manifest::{self, Manifest, RepoEdit, RepoEntry};
use crate::spec;

/// Run `grm remove`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: RemoveArgs, ctx: &Context) -> Result<ExitCode> {
    let manifest_path = manifest::discover(ctx.manifest_path.as_deref())?;
    let mut manifest = manifest::load(&manifest_path)?;

    let index = find_entry(&manifest, &args.spec)?;
    // `resolve` returns one repo per entry, in order, so `index` lines up.
    let resolved = manifest.resolve()?;
    let repo = resolved
        .get(index)
        .context("manifest entry missing after resolve")?;
    let checkout = repo.path.clone();

    // Decide about deletion first, so a "no" changes nothing at all.
    let delete = if args.delete && checkout.exists() {
        if !args.force {
            let problems = unsafe_to_delete(&checkout)?;
            if !problems.is_empty() {
                bail!(
                    "refusing to delete {}: it has {} (use --force to delete anyway)",
                    checkout.display(),
                    problems.join(", ")
                );
            }
        }
        if !args.yes && !commands::confirm(&format!("Delete {}?", checkout.display()))? {
            println!("Aborted; nothing changed.");
            return Ok(ExitCode::SUCCESS);
        }
        true
    } else {
        if args.delete {
            println!("{} does not exist; nothing to delete", checkout.display());
        }
        false
    };

    let removed = manifest.repos.remove(index);
    manifest::save_edit(&manifest, &manifest_path, RepoEdit::Remove(index))?;
    println!(
        "Removed {} from {}",
        entry_text(&removed),
        manifest_path.display()
    );

    if delete {
        fs::remove_dir_all(&checkout)
            .with_context(|| format!("deleting {}", checkout.display()))?;
        println!("Deleted {}", checkout.display());
    } else if checkout.exists() {
        println!(
            "The checkout at {} was kept (use --delete to remove it)",
            checkout.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Index into `manifest.repos` of the single entry `query` refers to.
fn find_entry(manifest: &Manifest, query: &str) -> Result<usize> {
    let resolved = manifest.resolve()?;

    // 1. Same repo by normalized URL (works for `owner/name`, URLs, ...).
    // RUST: `if let Ok(x) = ...` ignores the error case: a query that isn't a
    // valid spec (e.g. just `cargo`) simply skips this step.
    if let Ok(wanted) = spec::parse(query, &manifest.defaults) {
        let key = spec::normalize_url(&wanted.url);
        // RUST: `.position` returns the index of the first item for which
        // the closure returns true (as an `Option<usize>`).
        if let Some(i) = resolved
            .iter()
            .position(|r| spec::normalize_url(&r.spec.url) == key)
        {
            return Ok(i);
        }
    }

    // 2. Fallback: repo name or the entry text as written.
    // RUST: `.enumerate()` pairs each item with its index, `.filter` keeps
    // items matching the closure, `.map` transforms them, and `.collect()`
    // gathers the results into a collection (here a `Vec`).
    let matches: Vec<usize> = resolved
        .iter()
        .enumerate()
        .filter(|(i, r)| r.spec.name == query || entry_text(&manifest.repos[*i]) == query)
        .map(|(i, _)| i)
        .collect();

    match matches.as_slice() {
        [] => bail!("no repo matching `{query}` in the manifest"),
        [only] => Ok(*only),
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|&i| format!("  {}", resolved[i].spec.url))
                .collect();
            bail!(
                "`{query}` matches several repos; use the full spec or URL:\n{}",
                names.join("\n")
            )
        }
    }
}

/// The spec string of an entry, as written in the manifest.
fn entry_text(entry: &RepoEntry) -> &str {
    match entry {
        RepoEntry::Short(s) => s,
        RepoEntry::Full { url, .. } => url,
    }
}

/// Reasons it would be unsafe to delete `path` (empty = safe).
fn unsafe_to_delete(path: &Path) -> Result<Vec<&'static str>> {
    if !path.join(".git").exists() {
        return Ok(vec![
            "no .git (not a git checkout, so its contents can't be checked)",
        ]);
    }
    git::unsaved_work(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-c")
            .arg("init.defaultBranch=main")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup(tmp: &Path, repos: &[&str]) -> (PathBuf, Context) {
        let path = tmp.join("grm.yml");
        let m = Manifest {
            root: tmp.join("root").display().to_string(),
            defaults: Default::default(),
            repos: repos
                .iter()
                .map(|s| RepoEntry::Short(s.to_string()))
                .collect(),
        };
        manifest::save(&m, &path).unwrap();
        (
            path.clone(),
            Context {
                manifest_path: Some(path),
            },
        )
    }

    fn args(spec: &str) -> RemoveArgs {
        RemoveArgs {
            spec: spec.to_string(),
            delete: false,
            force: false,
            yes: true,
        }
    }

    fn remaining(path: &Path) -> Vec<RepoEntry> {
        manifest::load(path).unwrap().repos
    }

    #[test]
    fn removes_by_url_form() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), &["alice/proj", "bob/other"]);
        assert_eq!(
            run(args("git@github.com:alice/proj.git"), &ctx).unwrap(),
            ExitCode::SUCCESS
        );
        assert_eq!(remaining(&path), vec![RepoEntry::Short("bob/other".into())]);
    }

    #[test]
    fn removes_by_name() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), &["alice/proj", "bob/other"]);
        assert_eq!(run(args("other"), &ctx).unwrap(), ExitCode::SUCCESS);
        assert_eq!(
            remaining(&path),
            vec![RepoEntry::Short("alice/proj".into())]
        );
    }

    #[test]
    fn errors_when_missing_or_ambiguous() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("grm.yml");
        let m = Manifest {
            root: tmp.path().join("root").display().to_string(),
            defaults: Default::default(),
            repos: vec![
                RepoEntry::Short("alice/proj".into()),
                RepoEntry::Full {
                    url: "bob/proj".into(),
                    path: Some("bob-proj".into()),
                    branch: None,
                    identity: None,
                },
            ],
        };
        manifest::save(&m, &path).unwrap();
        assert!(find_entry(&m, "nope").is_err());
        let err = find_entry(&m, "proj").unwrap_err();
        assert!(err.to_string().contains("several"), "{err}");
        assert_eq!(find_entry(&m, "bob/proj").unwrap(), 1);
    }

    #[test]
    fn delete_refuses_dirty_unless_forced() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), &["alice/proj"]);
        let checkout = tmp.path().join("root/proj");
        fs::create_dir_all(&checkout).unwrap();
        run_git(&checkout, &["init", "-q"]);
        fs::write(checkout.join("work.txt"), "unsaved").unwrap();

        let mut a = args("alice/proj");
        a.delete = true;
        let err = run(a, &ctx).unwrap_err();
        assert!(err.to_string().contains("uncommitted"), "{err}");
        assert!(checkout.exists());
        assert_eq!(remaining(&path).len(), 1);

        let mut a = args("alice/proj");
        a.delete = true;
        a.force = true;
        assert_eq!(run(a, &ctx).unwrap(), ExitCode::SUCCESS);
        assert!(!checkout.exists());
        assert!(remaining(&path).is_empty());
    }

    #[test]
    fn delete_of_missing_checkout_still_removes_entry() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), &["alice/proj"]);
        let mut a = args("alice/proj");
        a.delete = true;
        assert_eq!(run(a, &ctx).unwrap(), ExitCode::SUCCESS);
        assert!(remaining(&path).is_empty());
    }
}
