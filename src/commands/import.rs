//! `grm import`: add existing checkouts to the manifest.
//!
//! - Scan `args.dir` (default: manifest root) with
//!   `git::find_repos(dir, --depth)`.
//! - Skip repos already in the manifest (same path or same normalized URL)
//!   and repos without an `origin` remote (these are reported).
//! - For the rest, build an entry:
//!   - `url`: the `owner/name` shorthand if it expands (under the manifest's
//!     default host + protocol) to the origin URL, otherwise the origin URL.
//!   - `path`: only if the checkout isn't at the default `{root}/{name}`;
//!     written relative to the root when it is under it.
//!   - `identity`: local `user.name` / `user.email`, only where they differ
//!     from `defaults.identity`.
//!
//!   No `path` and no `identity` → `RepoEntry::Short`, else `RepoEntry::Full`.
//! - `--dry-run`: print only. Otherwise save and print a summary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result};

use crate::cli::ImportArgs;
use crate::commands::Context;
use crate::git;
use crate::manifest::{self, Identity, Manifest, RepoEntry};
use crate::spec;

/// What an import scan found.
#[derive(Debug, Default)]
struct ImportPlan {
    /// New entries, with the checkout each one came from.
    added: Vec<(PathBuf, RepoEntry)>,
    /// Checkouts already covered by the manifest (or found twice in the scan).
    already_listed: Vec<PathBuf>,
    /// Checkouts with no `origin` remote.
    no_origin: Vec<PathBuf>,
    /// Checkouts whose origin couldn't be parsed, with the reason.
    invalid: Vec<(PathBuf, String)>,
}

/// Run `grm import`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: ImportArgs, ctx: &Context) -> Result<ExitCode> {
    let manifest_path = manifest::discover(ctx.manifest_path.as_deref())?;
    let mut manifest = manifest::load(&manifest_path)?;

    let dir = match args.dir {
        Some(d) => d,
        None => manifest.root_dir()?,
    };
    let repos = git::find_repos(&dir, args.depth)
        .with_context(|| format!("scanning {} for git repos", dir.display()))?;
    let plan = plan_import(&manifest, &repos)?;

    for path in &plan.no_origin {
        println!("skip {} (no `origin` remote)", path.display());
    }
    for (path, reason) in &plan.invalid {
        println!("skip {} ({reason})", path.display());
    }
    if !plan.already_listed.is_empty() {
        println!(
            "{} repo(s) already in the manifest were skipped",
            plan.already_listed.len()
        );
    }
    for (path, entry) in &plan.added {
        println!("+ {}  ({})", describe(entry), path.display());
    }

    let count = plan.added.len();
    if args.dry_run {
        println!("Dry run: would add {count} repo(s); the manifest was not changed.");
        return Ok(ExitCode::SUCCESS);
    }
    if count == 0 {
        println!("Nothing to import.");
        return Ok(ExitCode::SUCCESS);
    }

    // RUST: `.into_iter()` consumes the vector, giving owned items, so the
    // entries can be moved into the manifest without cloning.
    manifest
        .repos
        .extend(plan.added.into_iter().map(|(_, entry)| entry));
    // Make sure the result is still a valid manifest before writing it.
    manifest
        .resolve()
        .context("the imported entries would make the manifest invalid")?;
    manifest::save(&manifest, &manifest_path)?;
    println!("Imported {count} repo(s) into {}", manifest_path.display());
    Ok(ExitCode::SUCCESS)
}

/// Decide what to do with each found checkout. Reads git config but changes
/// nothing.
fn plan_import(manifest: &Manifest, repos: &[PathBuf]) -> Result<ImportPlan> {
    let root = manifest.root_dir()?;
    let existing = manifest.resolve()?;
    let default_identity = manifest.defaults.identity.clone().unwrap_or_default();

    // RUST: `HashSet` is a set with fast "contains" checks. `collect()` can
    // build one directly from an iterator.
    let mut known_paths: HashSet<PathBuf> = existing.iter().map(|r| r.path.clone()).collect();
    let mut known_urls: HashSet<String> = existing
        .iter()
        .map(|r| spec::normalize_url(&r.spec.url))
        .collect();

    let mut plan = ImportPlan::default();
    for repo in repos {
        if known_paths.contains(repo) {
            plan.already_listed.push(repo.clone());
            continue;
        }
        let Some(origin) = git::remote_url(repo)? else {
            plan.no_origin.push(repo.clone());
            continue;
        };
        let parsed = match spec::parse(&origin, &manifest.defaults) {
            Ok(p) => p,
            Err(e) => {
                plan.invalid.push((repo.clone(), format!("{e:#}")));
                continue;
            }
        };
        // RUST: `HashSet::insert` returns `false` if the value was already
        // there, which doubles as our "seen before?" check.
        if !known_urls.insert(spec::normalize_url(&parsed.url)) {
            plan.already_listed.push(repo.clone());
            continue;
        }
        known_paths.insert(repo.clone());

        let url = shorthand_for(&origin, &parsed, manifest).unwrap_or(origin);
        let path = if *repo == root.join(&parsed.name) {
            None
        } else {
            Some(relative_to_root(repo, &root))
        };
        let identity = identity_overrides(repo, &default_identity)?;

        let entry = if path.is_none() && identity.is_none() {
            RepoEntry::Short(url)
        } else {
            RepoEntry::Full {
                url,
                path,
                branch: None,
                identity,
            }
        };
        plan.added.push((repo.clone(), entry));
    }
    Ok(plan)
}

/// `owner/name` if that shorthand expands (under the manifest defaults) to
/// the same URL as `origin`, ignoring a trailing `.git`.
fn shorthand_for(origin: &str, parsed: &spec::RepoSpec, manifest: &Manifest) -> Option<String> {
    let short = format!("{}/{}", parsed.owner, parsed.name);
    let expanded = spec::parse(&short, &manifest.defaults).ok()?;
    let strip = |s: &str| s.trim().trim_end_matches(".git").to_string();
    (strip(&expanded.url) == strip(origin)).then_some(short)
}

/// `path` relative to `root` if it is under it, else the absolute path.
fn relative_to_root(path: &Path, root: &Path) -> String {
    // RUST: `strip_prefix` returns `Err` when `path` isn't under `root`;
    // `map_or(default, f)` handles both cases in one expression.
    path.strip_prefix(root)
        .map_or(path, |rel| rel)
        .display()
        .to_string()
}

/// Local `user.name` / `user.email` values that differ from `defaults`, as
/// an identity override (`None` if nothing differs).
fn identity_overrides(repo: &Path, defaults: &Identity) -> Result<Option<Identity>> {
    let differs = |local: Option<String>, default: &Option<String>| {
        local.filter(|value| Some(value) != default.as_ref())
    };
    let name = differs(git::config_get(repo, "user.name")?, &defaults.name);
    let email = differs(git::config_get(repo, "user.email")?, &defaults.email);
    if name.is_none() && email.is_none() {
        return Ok(None);
    }
    Ok(Some(Identity {
        name,
        email,
        signing: None,
    }))
}

/// One-line description of an entry for printing.
fn describe(entry: &RepoEntry) -> String {
    match entry {
        RepoEntry::Short(s) => s.clone(),
        RepoEntry::Full {
            url,
            path,
            identity,
            ..
        } => {
            let mut parts = vec![url.clone()];
            if let Some(p) = path {
                parts.push(format!("path: {p}"));
            }
            if let Some(id) = identity {
                if let Some(n) = &id.name {
                    parts.push(format!("name: {n}"));
                }
                if let Some(e) = &id.email {
                    parts.push(format!("email: {e}"));
                }
            }
            parts.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
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

    /// `git init` at `path`, optionally with an origin.
    fn checkout(path: &Path, origin: Option<&str>) {
        fs::create_dir_all(path).unwrap();
        run_git(path, &["init", "-q"]);
        if let Some(url) = origin {
            run_git(path, &["remote", "add", "origin", url]);
        }
    }

    fn setup(tmp: &Path, repos: Vec<RepoEntry>) -> (PathBuf, PathBuf, Context) {
        let path = tmp.join("grm.yml");
        let root = tmp.join("root");
        let m = Manifest {
            root: root.display().to_string(),
            defaults: manifest::Defaults {
                identity: Some(Identity {
                    name: Some("Default".into()),
                    email: Some("d@example.com".into()),
                    signing: None,
                }),
                ..Default::default()
            },
            repos,
        };
        manifest::save(&m, &path).unwrap();
        let ctx = Context {
            manifest_path: Some(path.clone()),
        };
        (path, root, ctx)
    }

    fn args(dry_run: bool) -> ImportArgs {
        ImportArgs {
            dir: None,
            depth: 2,
            dry_run,
        }
    }

    #[test]
    fn imports_with_shorthand_path_and_identity() {
        let tmp = TempDir::new().unwrap();
        let (path, root, ctx) = setup(tmp.path(), vec![RepoEntry::Short("x/listed".into())]);

        checkout(
            &root.join("cargo"),
            Some("git@github.com:rust-lang/cargo.git"),
        );
        checkout(
            &root.join("work/api"),
            Some("https://gitlab.com/work/api.git"),
        );
        checkout(&root.join("scratch"), None);
        checkout(&root.join("listed"), Some("git@github.com:x/listed.git"));
        let custom = root.join("custom");
        checkout(&custom, Some("git@github.com:me/thing.git"));
        run_git(&custom, &["config", "user.email", "me@work.example"]);
        run_git(&custom, &["config", "user.name", "Default"]);

        // Dry run changes nothing.
        assert_eq!(run(args(true), &ctx).unwrap(), ExitCode::SUCCESS);
        assert_eq!(manifest::load(&path).unwrap().repos.len(), 1);

        assert_eq!(run(args(false), &ctx).unwrap(), ExitCode::SUCCESS);
        let repos = manifest::load(&path).unwrap().repos;
        assert_eq!(
            repos,
            vec![
                RepoEntry::Short("x/listed".into()),
                RepoEntry::Short("rust-lang/cargo".into()),
                RepoEntry::Full {
                    url: "me/thing".into(),
                    path: Some("custom".into()),
                    branch: None,
                    identity: Some(Identity {
                        name: None,
                        email: Some("me@work.example".into()),
                        signing: None,
                    }),
                },
                RepoEntry::Full {
                    url: "https://gitlab.com/work/api.git".into(),
                    path: Some("work/api".into()),
                    branch: None,
                    identity: None,
                },
            ]
        );

        // Importing again adds nothing.
        assert_eq!(run(args(false), &ctx).unwrap(), ExitCode::SUCCESS);
        assert_eq!(manifest::load(&path).unwrap().repos.len(), 4);
    }

    #[test]
    fn https_origin_is_not_shorthand_under_ssh_defaults() {
        let tmp = TempDir::new().unwrap();
        let (_, root, _) = setup(tmp.path(), vec![]);
        checkout(
            &root.join("cargo"),
            Some("https://github.com/rust-lang/cargo"),
        );
        let m = manifest::load(&tmp.path().join("grm.yml")).unwrap();
        let plan = plan_import(&m, &[root.join("cargo")]).unwrap();
        assert_eq!(
            plan.added[0].1,
            RepoEntry::Short("https://github.com/rust-lang/cargo".into())
        );
    }

    #[test]
    fn duplicate_checkouts_are_imported_once() {
        let tmp = TempDir::new().unwrap();
        let (_, root, _) = setup(tmp.path(), vec![]);
        checkout(
            &root.join("a/cargo"),
            Some("git@github.com:rust-lang/cargo.git"),
        );
        checkout(
            &root.join("b/cargo"),
            Some("git@github.com:rust-lang/cargo"),
        );
        let m = manifest::load(&tmp.path().join("grm.yml")).unwrap();
        let plan = plan_import(&m, &[root.join("a/cargo"), root.join("b/cargo")]).unwrap();
        assert_eq!(plan.added.len(), 1);
        assert_eq!(plan.already_listed, vec![root.join("b/cargo")]);
    }

    #[test]
    fn path_outside_root_is_absolute() {
        let root = Path::new("/r");
        assert_eq!(relative_to_root(Path::new("/r/a/b"), root), "a/b");
        assert_eq!(relative_to_root(Path::new("/other/x"), root), "/other/x");
    }
}
