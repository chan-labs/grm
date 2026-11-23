//! Compare the manifest against what is actually on disk ("drift detection").
//!
//! Used by `status` (to report), `sync` (to decide what to fix) and `prune`
//! (via `Report::extras`).
//!
//! Everything here is read-only: it runs `git config --get`, `git status`
//! and friends, but never changes a repo and never fetches.

use std::fs;
use std::path::{Path, PathBuf};

// RUST: `use anyhow::Context as _` imports the trait (so `.context(...)` works
// on `Result`s) without bringing the name `Context` into scope.
use anyhow::{Context as _, Result};

use crate::git;
use crate::manifest::{Manifest, ResolvedRepo};
use crate::spec;

/// How deep below the root to look for extra repos. 3 covers both the flat
/// `{root}/{name}` layout and custom nested paths like `host/owner/name`.
const EXTRA_SCAN_DEPTH: usize = 3;

/// One way a checked-out repo differs from the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// A local git config key (from `Identity::to_git_config`) has the wrong
    /// value. `expected: None` means it should be unset.
    Config {
        key: String,
        actual: Option<String>,
        expected: Option<String>,
    },
    /// `origin` points somewhere else (compared with `spec::normalize_url`),
    /// or is missing (`actual: None`).
    Remote {
        actual: Option<String>,
        expected: String,
    },
    /// Uncommitted changes / untracked files. Informational: `sync` doesn't fix it.
    Dirty,
    /// HEAD differs from its upstream. Informational: `sync` doesn't fix it.
    AheadBehind { ahead: u32, behind: u32 },
}

impl Drift {
    /// True for drift that is only reported (`Dirty`, `AheadBehind`): it is
    /// the user's own work, not something the manifest controls, so it
    /// doesn't count towards [`Report::has_drift`] and `sync` leaves it alone.
    pub fn is_informational(&self) -> bool {
        matches!(self, Drift::Dirty | Drift::AheadBehind { .. })
    }
}

/// Overall state of one manifest repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    /// Path doesn't exist (or isn't a git repo) – needs cloning.
    Missing,
    /// Matches the manifest.
    Ok,
    /// Exists but differs; never an empty Vec.
    Drifted(Vec<Drift>),
}

/// State of a single manifest repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoReport {
    pub repo: ResolvedRepo,
    pub state: RepoState,
}

impl RepoReport {
    /// True if this repo needs `sync` to do something: it is `Missing`, or
    /// `Drifted` with at least one non-informational drift.
    pub fn needs_sync(&self) -> bool {
        match &self.state {
            RepoState::Missing => true,
            RepoState::Ok => false,
            RepoState::Drifted(drifts) => drifts.iter().any(|d| !d.is_informational()),
        }
    }
}

/// Result of inspecting the whole manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One entry per manifest repo, in manifest order.
    pub repos: Vec<RepoReport>,
    /// Git repos found under the root (`git::find_repos`, depth 3 to cover
    /// `host/owner/name`) that no manifest repo resolves to. Sorted.
    pub extras: Vec<PathBuf>,
}

impl Report {
    /// True if reality doesn't match the manifest: any repo is `Missing`,
    /// any repo is `Drifted` in a way `sync` would fix (wrong config or
    /// remote), or there are `extras`. `status` exits 1 when this is true.
    ///
    /// Informational drift alone (uncommitted changes, ahead/behind; see
    /// [`Drift::is_informational`]) does *not* count: that's normal day-to-day
    /// work, and counting it would make `grm status` fail whenever you're
    /// mid-edit.
    pub fn has_drift(&self) -> bool {
        // RUST: iterator adapters like `.any(...)` take a closure (`|r| ...`),
        // an inline anonymous function. A named function or method works too:
        // `RepoReport::needs_sync` is short for `|r| r.needs_sync()`.
        !self.extras.is_empty() || self.repos.iter().any(RepoReport::needs_sync)
    }
}

/// Resolve the manifest and inspect every repo on disk (read-only; no fetch).
///
/// For each existing repo: compare `identity.to_git_config()` against
/// `git::config_get`, `origin` against `spec.url` (normalized), and add
/// `Dirty` / `AheadBehind` (only when non-zero) informational drift.
pub fn inspect(m: &Manifest) -> Result<Report> {
    let resolved = m.resolve().context("resolving manifest repos")?;

    let mut repos = Vec::with_capacity(resolved.len());
    // RUST: `for x in vec` calls `vec.into_iter()`, which consumes the Vec and
    // hands us owned `ResolvedRepo`s that we can move into the reports
    // without cloning. (`for x in &vec` would borrow each item instead.)
    for repo in resolved {
        let state = inspect_repo(&repo)?;
        repos.push(RepoReport { repo, state });
    }

    let root = m.root_dir()?;
    let found = git::find_repos(&root, EXTRA_SCAN_DEPTH)
        .with_context(|| format!("scanning {} for repos", root.display()))?;
    let managed: Vec<PathBuf> = repos
        .iter()
        .map(|r| absolute(&r.repo.path))
        // RUST: `collect::<Result<Vec<_>>>()` turns an iterator of `Result`s
        // into one `Result`: the first error, or a Vec of all the values.
        .collect::<Result<Vec<_>>>()?;
    let extras = find_extras(found, &managed);

    Ok(Report { repos, extras })
}

/// Inspect a single resolved repo (read-only). Used by [`inspect`], and by
/// `sync` to re-check a repo right after cloning it.
pub fn inspect_repo(repo: &ResolvedRepo) -> Result<RepoState> {
    if !is_git_repo(&repo.path) {
        return Ok(RepoState::Missing);
    }
    let path = repo.path.as_path();
    // A closure that adds "which repo" to every error below.
    let ctx = || format!("inspecting {}", path.display());

    let mut drifts = Vec::new();

    for (key, expected) in repo.identity.to_git_config() {
        let actual = git::config_get(path, &key).with_context(ctx)?;
        if actual != expected {
            drifts.push(Drift::Config {
                key,
                actual,
                expected,
            });
        }
    }

    let actual_url = git::remote_url(path).with_context(ctx)?;
    // RUST: `Option::is_some_and(f)` is false for `None`, else `f(value)`.
    let url_matches = actual_url
        .as_deref()
        .is_some_and(|actual| spec::normalize_url(actual) == spec::normalize_url(&repo.spec.url));
    if !url_matches {
        drifts.push(Drift::Remote {
            actual: actual_url,
            expected: repo.spec.url.clone(),
        });
    }

    if git::is_dirty(path).with_context(ctx)? {
        drifts.push(Drift::Dirty);
    }
    if let Some((ahead, behind)) = git::ahead_behind(path).with_context(ctx)?
        && (ahead > 0 || behind > 0)
    {
        drifts.push(Drift::AheadBehind { ahead, behind });
    }

    Ok(if drifts.is_empty() {
        RepoState::Ok
    } else {
        RepoState::Drifted(drifts)
    })
}

/// A directory counts as a git repo if it has a `.git` entry (directory, or
/// file for worktrees/submodules) — the same rule `git::find_repos` uses.
fn is_git_repo(path: &Path) -> bool {
    fs::symlink_metadata(path.join(".git")).is_ok()
}

/// `std::path::absolute` with error context.
fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("failed to make {} absolute", path.display()))
}

/// Keep the `found` repos that aren't a managed repo, inside one (e.g. a
/// nested checkout or submodule), or a parent of one (a repo that a custom
/// `path:` lives inside).
fn find_extras(found: Vec<PathBuf>, managed: &[PathBuf]) -> Vec<PathBuf> {
    let mut extras: Vec<PathBuf> = found
        .into_iter()
        .filter(|f| {
            // RUST: `Path::starts_with` compares whole path components, so
            // `/src/foo` does not "start with" `/src/fo`.
            !managed.iter().any(|m| f.starts_with(m) || m.starts_with(f))
        })
        .collect();
    extras.sort();
    extras
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Defaults, Identity, RepoEntry};
    use std::process::Command;
    use tempfile::TempDir;

    /// Run git in `dir` for test setup, isolated from the developer's config.
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

    /// A bare remote at `<tmp>/remotes/<name>.git` with one commit, returning
    /// its `file://` URL.
    fn bare_remote(tmp: &Path, name: &str) -> String {
        let bare = tmp.join("remotes").join(format!("{name}.git"));
        let seed = tmp.join("seeds").join(name);
        fs::create_dir_all(&bare).expect("mkdir");
        fs::create_dir_all(&seed).expect("mkdir");
        run(&bare, &["init", "-q", "--bare"]);
        run(&seed, &["init", "-q"]);
        run(
            &seed,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        run(&seed, &["push", "-q", bare.to_str().expect("utf8"), "main"]);
        format!("file://{}", bare.display())
    }

    fn manifest(root: &Path, urls: &[&str]) -> Manifest {
        Manifest {
            root: root.display().to_string(),
            defaults: Defaults {
                identity: Some(Identity {
                    name: Some("Jane".into()),
                    email: Some("jane@example.com".into()),
                    signing: None,
                }),
                ..Defaults::default()
            },
            repos: urls
                .iter()
                .map(|u| RepoEntry::Short(u.to_string()))
                .collect(),
        }
    }

    #[test]
    fn missing_ok_and_drifted() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("root");
        fs::create_dir_all(&root).expect("mkdir");
        let a = bare_remote(tmp.path(), "a");
        let b = bare_remote(tmp.path(), "b");
        let c = bare_remote(tmp.path(), "c");
        let m = manifest(&root, &[&a, &b, &c]);

        // `a` stays missing. `b` is cloned and configured correctly.
        git::clone(&b, &root.join("b"), None).expect("clone");
        git::config_set(&root.join("b"), "user.name", "Jane").expect("set");
        git::config_set(&root.join("b"), "user.email", "jane@example.com").expect("set");
        // `c` has a wrong email, a stray gpgsign, the wrong origin and a new file.
        let cpath = root.join("c");
        git::clone(&c, &cpath, None).expect("clone");
        git::config_set(&cpath, "user.name", "Jane").expect("set");
        git::config_set(&cpath, "user.email", "old@example.com").expect("set");
        git::config_set(&cpath, "commit.gpgsign", "true").expect("set");
        git::set_remote_url(&cpath, &a).expect("set url");
        fs::write(cpath.join("new.txt"), "hi").expect("write");

        let report = inspect(&m).expect("inspect");
        assert_eq!(report.repos[0].state, RepoState::Missing);
        assert_eq!(report.repos[1].state, RepoState::Ok);
        assert_eq!(
            report.repos[2].state,
            RepoState::Drifted(vec![
                Drift::Config {
                    key: "user.email".into(),
                    actual: Some("old@example.com".into()),
                    expected: Some("jane@example.com".into()),
                },
                Drift::Config {
                    key: "commit.gpgsign".into(),
                    actual: Some("true".into()),
                    expected: None,
                },
                Drift::Remote {
                    actual: Some(a.clone()),
                    expected: c.clone(),
                },
                Drift::Dirty,
            ])
        );
        assert!(report.extras.is_empty());
        assert!(report.has_drift());
    }

    #[test]
    fn informational_drift_is_not_has_drift() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("root");
        let a = bare_remote(tmp.path(), "a");
        let m = manifest(&root, &[&a]);
        let apath = root.join("a");
        git::clone(&a, &apath, None).expect("clone");
        git::config_set(&apath, "user.name", "Jane").expect("set");
        git::config_set(&apath, "user.email", "jane@example.com").expect("set");
        fs::write(apath.join("wip.txt"), "wip").expect("write");

        let report = inspect(&m).expect("inspect");
        assert_eq!(
            report.repos[0].state,
            RepoState::Drifted(vec![Drift::Dirty])
        );
        assert!(!report.has_drift());
    }

    #[test]
    fn extras_are_found_but_not_nested_or_parent_repos() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("root");
        let a = bare_remote(tmp.path(), "a");
        let x = bare_remote(tmp.path(), "x");
        let m = manifest(&root, &[&a]);
        git::clone(&a, &root.join("a"), None).expect("clone");
        // Nested inside a managed repo: not an extra.
        git::clone(&x, &root.join("a/vendor/x"), None).expect("clone");
        // Unrelated checkout: an extra.
        git::clone(&x, &root.join("other/x"), None).expect("clone");

        let report = inspect(&m).expect("inspect");
        let root_abs = std::path::absolute(&root).expect("abs");
        assert_eq!(report.extras, vec![root_abs.join("other/x")]);
        assert!(report.has_drift());
    }

    #[test]
    fn find_extras_excludes_parents_and_children() {
        let managed = vec![PathBuf::from("/r/a"), PathBuf::from("/r/outer/inner")];
        let found = vec![
            PathBuf::from("/r/a"),
            PathBuf::from("/r/a/sub"),
            PathBuf::from("/r/ab"),
            PathBuf::from("/r/outer"),
            PathBuf::from("/r/z"),
        ];
        assert_eq!(
            find_extras(found, &managed),
            vec![PathBuf::from("/r/ab"), PathBuf::from("/r/z")]
        );
    }
}
