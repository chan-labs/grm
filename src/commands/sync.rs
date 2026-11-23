//! `grm sync`: make disk match the manifest.
//!
//! - `state::inspect` the manifest.
//! - `Missing` repos: `git::clone` (up to `--jobs` in parallel, using
//!   `std::thread::scope`), then apply identity config.
//! - Every present repo: fix `Config` (set/unset) and `Remote` drift; just
//!   report `Dirty` / `AheadBehind`. Working-tree content is never touched
//!   and nothing is pulled.
//! - `--prune`: then behave like `grm prune` (honouring `--yes` and
//!   `--dry-run`; there is no `--force`, so repos with unsaved work are
//!   always skipped).
//! - `--dry-run`: print planned actions only.
//! - A failing repo is reported and the others still run. Exit 1 if any
//!   repo (or prune deletion) failed.

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Context as _, Result, anyhow};

use crate::cli::SyncArgs;
use crate::commands::Context;
use crate::commands::prune::{PruneOptions, prune_extras};
use crate::commands::status::{Painter, describe_drift};
use crate::git;
use crate::manifest::{self, ResolvedRepo};
use crate::state::{self, Drift, RepoState, Report};

/// Run `grm sync`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: SyncArgs, ctx: &Context) -> Result<ExitCode> {
    let path = manifest::discover(ctx.manifest_path.as_deref())?;
    let m = manifest::load(&path)?;
    let report = state::inspect(&m).context("inspecting repos")?;
    let p = Painter::new();

    let mut ok = if args.dry_run {
        print_plan(&report, &p);
        true
    } else {
        apply(&report, args.jobs, &p) == 0
    };

    if args.prune {
        println!();
        let opts = PruneOptions {
            yes: args.yes,
            force: false,
            dry_run: args.dry_run,
        };
        // RUST: `&=` is "and-assign": `ok` stays true only if both are true.
        ok &= prune_extras(&report.extras, opts)?;
    }

    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `--dry-run`: say what `apply` would do, without doing it.
fn print_plan(report: &Report, p: &Painter) {
    for r in &report.repos {
        let name = &r.repo.spec.name;
        match &r.state {
            RepoState::Missing => println!(
                "{} {name}  would clone {} into {}",
                p.ok("+"),
                r.repo.spec.url,
                r.repo.path.display()
            ),
            RepoState::Ok => println!("{} {name}  up to date", p.ok("✓")),
            RepoState::Error(msg) => {
                println!("{} {name}  would skip (can't inspect): {msg}", p.err("✗"))
            }
            RepoState::Drifted(drifts) => {
                if r.needs_sync() {
                    println!("{} {name}  would update", p.warn("~"));
                } else {
                    println!("{} {name}  up to date", p.ok("✓"));
                }
                print_drifts(drifts, p);
            }
        }
    }
    println!("\n(dry run: nothing was changed)");
}

/// Print each drift indented; informational ones are dimmed and marked as
/// left alone.
fn print_drifts(drifts: &[Drift], p: &Painter) {
    for d in drifts {
        if d.is_informational() {
            println!(
                "    {}",
                p.dim(format!("{} (left alone)", describe_drift(d)))
            );
        } else {
            println!("    {}", describe_drift(d));
        }
    }
}

/// Clone missing repos, then fix config/remote drift on every present repo.
/// Prints one block per repo, in manifest order. Returns the failure count.
fn apply(report: &Report, jobs: usize, p: &Painter) -> usize {
    // RUST: `filter` keeps items matching a predicate; `map` transforms them.
    let missing: Vec<&ResolvedRepo> = report
        .repos
        .iter()
        .filter(|r| r.state == RepoState::Missing)
        .map(|r| &r.repo)
        .collect();
    if !missing.is_empty() {
        println!("cloning {} repo(s)...", missing.len());
    }
    let mut clone_results = clone_all(&missing, jobs).into_iter();

    let (mut cloned, mut updated, mut unchanged, mut failed) = (0, 0, 0, 0);
    for r in &report.repos {
        let name = &r.repo.spec.name;
        let was_missing = r.state == RepoState::Missing;

        // RUST: `if` is an expression, so its value can be assigned; then
        // `and_then` runs the next fallible step only if this one was `Ok`.
        let result: Result<Vec<Drift>> = if was_missing {
            // `clone_all` returns results in the same order as `missing`.
            match clone_results.next() {
                Some(Ok(())) => state::inspect_repo(&r.repo).map(|s| drifts_of(&s)),
                Some(Err(e)) => Err(e),
                None => Err(anyhow!("internal error: no clone result")),
            }
        } else if let RepoState::Error(msg) = &r.state {
            // Couldn't inspect it, so don't guess what to change: report it as
            // a failure and carry on with the other repos.
            Err(anyhow!("can't inspect repo: {msg}"))
        } else {
            Ok(drifts_of(&r.state))
        }
        .and_then(|drifts| fix(&r.repo, &drifts).map(|()| drifts));

        match result {
            Err(e) => {
                failed += 1;
                println!("{} {name}  failed: {e:#}", p.err("✗"));
            }
            Ok(drifts) => {
                let fixed = drifts.iter().any(|d| !d.is_informational());
                if was_missing {
                    cloned += 1;
                    println!(
                        "{} {name}  cloned into {}",
                        p.ok("+"),
                        r.repo.path.display()
                    );
                } else if fixed {
                    updated += 1;
                    println!("{} {name}  updated", p.warn("~"));
                } else {
                    unchanged += 1;
                    println!("{} {name}  up to date", p.ok("✓"));
                }
                print_drifts(&drifts, p);
            }
        }
    }

    println!("\n{cloned} cloned, {updated} updated, {unchanged} up to date, {failed} failed");
    failed
}

/// The drift list inside a state (empty for `Ok` / `Missing` / `Error`).
fn drifts_of(state: &RepoState) -> Vec<Drift> {
    match state {
        RepoState::Drifted(d) => d.clone(),
        RepoState::Ok | RepoState::Missing | RepoState::Error(_) => Vec::new(),
    }
}

/// Fix the config and remote drifts of one present repo. Informational
/// drift is ignored. Stops at the first error.
fn fix(repo: &ResolvedRepo, drifts: &[Drift]) -> Result<()> {
    let path = repo.path.as_path();
    for d in drifts {
        match d {
            Drift::Config {
                key,
                expected: Some(value),
                ..
            } => git::config_set(path, key, value)?,
            Drift::Config {
                key,
                expected: None,
                ..
            } => git::config_unset(path, key)?,
            Drift::Remote { expected, .. } => git::set_remote_url(path, expected)?,
            Drift::Dirty | Drift::AheadBehind { .. } => {}
        }
    }
    Ok(())
}

/// What one clone worker thread reports: `(index into repos, result)` pairs.
// RUST: `type` gives an existing type a shorter alias name.
type WorkerResults = Vec<(usize, Result<()>)>;

/// Clone every repo in `repos`, at most `jobs` at a time. Returns one result
/// per repo, in the same order.
fn clone_all(repos: &[&ResolvedRepo], jobs: usize) -> Vec<Result<()>> {
    let workers = jobs.clamp(1, repos.len().max(1));
    // A shared counter: each worker atomically takes the next index to clone.
    // RUST: atomics can be updated safely from many threads without a lock.
    let next = AtomicUsize::new(0);

    // RUST: `thread::scope` lets the spawned threads borrow local variables
    // (`repos`, `next`); Rust guarantees they all finish before the scope
    // ends, so the borrows can't outlive the data.
    let per_worker: Vec<thread::Result<WorkerResults>> = thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                // RUST: `move` makes the closure take ownership of what it
                // captures; `next` here is a `&AtomicUsize`, so only the
                // reference is moved.
                let next = &next;
                s.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        // RUST: `let Some(x) = ... else { ... }` binds `x` or
                        // runs the `else` block, which must leave the loop.
                        let Some(repo) = repos.get(i) else { break };
                        let res = git::clone(&repo.spec.url, &repo.path, repo.branch.as_deref())
                            .with_context(|| format!("cloning {}", repo.spec.url));
                        done.push((i, res));
                    }
                    done
                })
            })
            .collect();
        // `join` waits for a thread; it's `Err` only if the thread panicked.
        handles.into_iter().map(|h| h.join()).collect()
    });

    // Put results back in input order; anything a panicked worker didn't
    // report becomes an error.
    let mut results: Vec<Option<Result<()>>> = (0..repos.len()).map(|_| None).collect();
    for (i, res) in per_worker.into_iter().flatten().flatten() {
        results[i] = Some(res);
    }
    results
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err(anyhow!("clone worker thread panicked"))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Defaults, Identity, Manifest, RepoEntry};
    use std::fs;
    use std::path::Path;
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
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn bare_remote(tmp: &Path, name: &str) -> String {
        let bare = tmp.join("remotes").join(format!("{name}.git"));
        let seed = tmp.join("seeds").join(name);
        fs::create_dir_all(&bare).expect("mkdir");
        fs::create_dir_all(&seed).expect("mkdir");
        run_git(&bare, &["init", "-q", "--bare"]);
        run_git(&seed, &["init", "-q"]);
        run_git(
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
        run_git(&seed, &["push", "-q", bare.to_str().expect("utf8"), "main"]);
        format!("file://{}", bare.display())
    }

    #[test]
    fn clones_missing_and_fixes_drift() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("root");
        let urls: Vec<String> = ["a", "b", "c"]
            .iter()
            .map(|n| bare_remote(tmp.path(), n))
            .collect();
        let m = Manifest {
            root: root.display().to_string(),
            defaults: Defaults {
                identity: Some(Identity {
                    name: Some("Jane".into()),
                    email: Some("jane@example.com".into()),
                    signing: None,
                }),
                ..Defaults::default()
            },
            repos: urls.iter().map(|u| RepoEntry::Short(u.clone())).collect(),
        };

        // `c` already exists with a wrong origin and a stray config key.
        let c = root.join("c");
        git::clone(&urls[0], &c, None).expect("clone");
        git::config_set(&c, "commit.gpgsign", "true").expect("set");
        fs::write(c.join("wip.txt"), "keep me").expect("write");

        let report = state::inspect(&m).expect("inspect");
        assert_eq!(apply(&report, 2, &Painter::plain()), 0);

        let after = state::inspect(&m).expect("inspect");
        assert!(!after.has_drift(), "{after:#?}");
        // The working tree was left alone.
        assert_eq!(
            fs::read_to_string(c.join("wip.txt")).expect("read"),
            "keep me"
        );
        assert_eq!(
            git::config_get(&root.join("a"), "user.email").expect("get"),
            Some("jane@example.com".into())
        );
    }

    #[test]
    fn failures_are_counted_and_others_continue() {
        let tmp = TempDir::new().expect("tempdir");
        let good = bare_remote(tmp.path(), "good");
        let bad = format!("file://{}/remotes/nope.git", tmp.path().display());
        let m = Manifest {
            root: tmp.path().join("root").display().to_string(),
            defaults: Defaults::default(),
            repos: vec![RepoEntry::Short(bad), RepoEntry::Short(good)],
        };
        let report = state::inspect(&m).expect("inspect");
        assert_eq!(apply(&report, 4, &Painter::plain()), 1);
        assert!(tmp.path().join("root/good/.git").exists());
    }

    #[test]
    fn clone_all_with_no_repos() {
        assert!(clone_all(&[], 4).is_empty());
    }
}
