//! `grm add`: add a repo to the manifest and clone it.
//!
//! - Load the manifest (`manifest::discover(ctx.manifest_path.as_deref())`).
//! - Parse `args.spec` with `spec::parse`; error if a repo with the same
//!   `spec::normalize_url` is already listed, or if the checkout path is
//!   already used by another entry (suggesting `--path`).
//! - Append `RepoEntry::Short(spec)` if no `--path/--name/--email/--branch`,
//!   else `RepoEntry::Full` (identity from `--name/--email`). Save.
//! - Unless `--no-clone`: clone to the resolved path (skipped if a checkout of
//!   the same repo is already there) and apply the merged identity's git
//!   config, printing each key that was set or unset.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};

use crate::cli::AddArgs;
use crate::commands::Context;
use crate::git;
use crate::manifest::{self, Identity, Manifest, RepoEdit, RepoEntry, ResolvedRepo};
use crate::spec;

/// Run `grm add`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: AddArgs, ctx: &Context) -> Result<ExitCode> {
    let manifest_path = manifest::discover(ctx.manifest_path.as_deref())?;
    let mut manifest = manifest::load(&manifest_path)?;

    let repo = add_entry(&mut manifest, &args)?;

    // Check the destination before touching the manifest, so a clash with
    // an unrelated directory doesn't leave a half-done `add` behind.
    let needs_clone = if args.no_clone {
        false
    } else {
        needs_clone(&repo)?
    };

    // `add_entry` pushed the new entry last, so that's the one to append.
    // RUST: `slice::from_ref` turns `&T` into a one-element `&[T]` without
    // copying. Both borrows of `manifest` here are shared (`&`), so they can
    // exist at the same time.
    let added = manifest.repos.last().context("no entry was added")?;
    manifest::save_edit(
        &manifest,
        &manifest_path,
        RepoEdit::Append(std::slice::from_ref(added)),
    )?;
    println!("Added {} to {}", args.spec, manifest_path.display());

    if args.no_clone {
        return Ok(ExitCode::SUCCESS);
    }

    if needs_clone {
        println!("Cloning {} into {}", repo.spec.url, repo.path.display());
        git::clone(&repo.spec.url, &repo.path, repo.branch.as_deref()).context(
            "the repo was added to the manifest but cloning failed; run `grm sync` to retry",
        )?;
    } else {
        println!("{} is already cloned", repo.path.display());
    }

    let changes = apply_identity(&repo.path, &repo.identity)?;
    if changes.is_empty() {
        println!("Git identity config already up to date");
    }
    for change in changes {
        println!("  {change}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Validate `args` against `manifest`, append the new entry and return it
/// resolved (absolute path, merged identity).
// RUST: ownership in function signatures. `run(args: AddArgs, ..)` above
// takes `args` by value (it now owns it). Here `&mut Manifest` is a mutable
// borrow (we may change the manifest but the caller still owns it) and
// `&AddArgs` a shared, read-only borrow. The compiler allows either one
// `&mut` or any number of `&` borrows at a time, never both.
fn add_entry(manifest: &mut Manifest, args: &AddArgs) -> Result<ResolvedRepo> {
    let spec = spec::parse(&args.spec, &manifest.defaults)?;
    let existing = manifest.resolve()?;
    let root = manifest.root_dir()?;

    // Duplicate repo?
    let key = spec::normalize_url(&spec.url);
    // RUST: `.iter().find(|r| ...)` returns the first element for which the
    // closure is true, as an `Option<&T>`.
    if let Some(dup) = existing
        .iter()
        .find(|r| spec::normalize_url(&r.spec.url) == key)
    {
        bail!(
            "{} is already in the manifest (as {}, at {})",
            args.spec,
            dup.spec.url,
            dup.path.display()
        );
    }

    // Path already taken by another entry?
    let path = match &args.path {
        Some(p) => root.join(manifest::expand_tilde(p)?),
        None => root.join(&spec.name),
    };
    if let Some(other) = existing.iter().find(|r| r.path == path) {
        let hint = if args.path.is_some() {
            "choose a different --path"
        } else {
            "pass --path to check it out somewhere else"
        };
        bail!(
            "{} is already used by {}; {hint}",
            path.display(),
            other.spec.url
        );
    }

    let identity = if args.name.is_some() || args.email.is_some() {
        Some(Identity {
            name: args.name.clone(),
            email: args.email.clone(),
            signing: None,
        })
    } else {
        None
    };
    let entry = if args.path.is_none() && args.branch.is_none() && identity.is_none() {
        RepoEntry::Short(args.spec.clone())
    } else {
        RepoEntry::Full {
            url: args.spec.clone(),
            path: args.path.clone(),
            branch: args.branch.clone(),
            identity,
        }
    };
    manifest.repos.push(entry);

    // Re-resolve so the new entry gets exactly the same treatment (path,
    // identity merge, validation) as every other command will give it.
    let mut resolved = manifest.resolve()?;
    // RUST: `.pop()` removes and returns the last element as an `Option`;
    // `.context` turns `None` into an error instead of panicking.
    resolved.pop().context("new entry missing after resolve")
}

/// Whether `repo.path` still needs `git clone`.
///
/// - missing or an empty directory → `true`
/// - a checkout whose `origin` is the same repo → `false`
/// - anything else → error (we won't clone over or adopt it silently)
fn needs_clone(repo: &ResolvedRepo) -> Result<bool> {
    let path = &repo.path;
    if !path.exists() {
        return Ok(true);
    }
    if path.is_dir()
        && path
            .read_dir()
            .with_context(|| format!("reading {}", path.display()))?
            .next()
            .is_none()
    {
        return Ok(true);
    }
    if !path.join(".git").exists() {
        bail!(
            "{} already exists and is not a git checkout; pass --path to use another location",
            path.display()
        );
    }
    match git::remote_url(path)? {
        Some(origin) if spec::normalize_url(&origin) == spec::normalize_url(&repo.spec.url) => {
            Ok(false)
        }
        Some(origin) => bail!(
            "{} is already a checkout of a different repo (origin {origin}); pass --path to use another location",
            path.display()
        ),
        None => bail!(
            "{} is already a git checkout without an `origin` remote; pass --path to use another location",
            path.display()
        ),
    }
}

/// Make the repo's local git config match `identity` (see
/// `Identity::to_git_config`): set keys whose value differs, unset keys that
/// should be absent. Returns a human-readable line per change.
fn apply_identity(repo: &Path, identity: &Identity) -> Result<Vec<String>> {
    let mut changes = Vec::new();
    for (key, wanted) in identity.to_git_config() {
        let current = git::config_get(repo, &key)?;
        // RUST: matching on a tuple of two `Option`s handles all four
        // combinations in one place.
        match (wanted, current) {
            (Some(want), Some(cur)) if want == cur => {}
            (Some(want), _) => {
                git::config_set(repo, &key, &want)?;
                changes.push(format!("set {key} = {want}"));
            }
            (None, Some(_)) => {
                git::config_unset(repo, &key)?;
                changes.push(format!("unset {key}"));
            }
            (None, None) => {}
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    /// Run git for test setup, isolated from the developer's global config.
    fn run_git(dir: &Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A bare remote at `<tmp>/remotes/<owner>/<name>.git` with one commit.
    fn bare_remote(tmp: &Path, owner: &str, name: &str) -> String {
        let bare = tmp.join("remotes").join(owner).join(format!("{name}.git"));
        fs::create_dir_all(&bare).unwrap();
        run_git(&bare, &["init", "-q", "--bare"]);
        let seed = tmp.join(format!("seed-{name}"));
        fs::create_dir_all(&seed).unwrap();
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
        run_git(&seed, &["push", "-q", bare.to_str().unwrap(), "main"]);
        bare.display().to_string()
    }

    /// Manifest with `root` = `<tmp>/root`, saved at `<tmp>/grm.yml`.
    fn setup(tmp: &Path, yaml_repos: &str) -> (PathBuf, Context) {
        let path = tmp.join("grm.yml");
        let root = tmp.join("root");
        fs::write(
            &path,
            format!(
                "root: {}\ndefaults:\n  identity:\n    name: Default\n    email: d@example.com\nrepos:\n{yaml_repos}",
                root.display()
            ),
        )
        .unwrap();
        let ctx = Context {
            manifest_path: Some(path.clone()),
        };
        (path, ctx)
    }

    fn args(spec: &str) -> AddArgs {
        AddArgs {
            spec: spec.to_string(),
            path: None,
            name: None,
            email: None,
            branch: None,
            no_clone: true,
        }
    }

    #[test]
    fn short_entry_without_overrides() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), "  []\n");
        assert_eq!(run(args("alice/proj"), &ctx).unwrap(), ExitCode::SUCCESS);
        let m = manifest::load(&path).unwrap();
        assert_eq!(m.repos, vec![RepoEntry::Short("alice/proj".into())]);
    }

    #[test]
    fn full_entry_with_overrides() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), "  []\n");
        let mut a = args("alice/proj");
        a.path = Some("work/proj".into());
        a.email = Some("w@example.com".into());
        assert_eq!(run(a, &ctx).unwrap(), ExitCode::SUCCESS);
        let m = manifest::load(&path).unwrap();
        assert_eq!(
            m.repos,
            vec![RepoEntry::Full {
                url: "alice/proj".into(),
                path: Some("work/proj".into()),
                branch: None,
                identity: Some(Identity {
                    name: None,
                    email: Some("w@example.com".into()),
                    signing: None,
                }),
            }]
        );
    }

    #[test]
    fn rejects_duplicate_by_normalized_url() {
        let tmp = TempDir::new().unwrap();
        let (_, ctx) = setup(tmp.path(), "  - alice/proj\n");
        let err = run(args("https://github.com/Alice/proj"), &ctx).unwrap_err();
        assert!(err.to_string().contains("already in the manifest"), "{err}");
    }

    #[test]
    fn rejects_path_collision_with_hint() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), "  - alice/proj\n");
        let err = run(args("bob/proj"), &ctx).unwrap_err();
        assert!(err.to_string().contains("--path"), "{err}");
        // Manifest unchanged.
        assert_eq!(manifest::load(&path).unwrap().repos.len(), 1);
    }

    #[test]
    fn clones_and_applies_identity_then_skips_second_clone() {
        let tmp = TempDir::new().unwrap();
        let url = bare_remote(tmp.path(), "alice", "proj");
        let (path, ctx) = setup(tmp.path(), "  []\n");
        let mut a = args(&url);
        a.no_clone = false;
        a.name = Some("Override".into());
        assert_eq!(run(a, &ctx).unwrap(), ExitCode::SUCCESS);

        let checkout = tmp.path().join("root/proj");
        assert!(checkout.join(".git").is_dir());
        assert_eq!(
            git::config_get(&checkout, "user.name").unwrap().as_deref(),
            Some("Override")
        );
        assert_eq!(
            git::config_get(&checkout, "user.email").unwrap().as_deref(),
            Some("d@example.com")
        );

        // Remove the entry but keep the checkout; re-adding adopts it.
        let mut m = manifest::load(&path).unwrap();
        m.repos.clear();
        manifest::save(&m, &path).unwrap();
        let mut a = args(&url);
        a.no_clone = false;
        assert_eq!(run(a, &ctx).unwrap(), ExitCode::SUCCESS);
        // Identity is now the defaults again.
        assert_eq!(
            git::config_get(&checkout, "user.name").unwrap().as_deref(),
            Some("Default")
        );
    }

    #[test]
    fn refuses_existing_non_repo_dir() {
        let tmp = TempDir::new().unwrap();
        let (path, ctx) = setup(tmp.path(), "  []\n");
        let dir = tmp.path().join("root/proj");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file"), "x").unwrap();
        let mut a = args("alice/proj");
        a.no_clone = false;
        assert!(run(a, &ctx).is_err());
        assert!(manifest::load(&path).unwrap().repos.is_empty());
    }
}
