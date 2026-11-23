//! `grm init`: create a new manifest.
//!
//! - Path: `ctx.manifest_path` (from `-f` / `GRM_FILE`) if set, else the
//!   global manifest `~/.config/grm/grm.yml` (`manifest::global_manifest_path`).
//!   We don't use `manifest::discover` here: it fails when no manifest exists
//!   yet, which is exactly the situation `init` is for.
//! - Refuse to overwrite an existing file unless `--force`.
//! - `root` = `--root` or `~/workspace`; defaults are `github.com` / `ssh`,
//!   with `defaults.identity` pre-filled from the global git config
//!   (`user.name` / `user.email`) when those are set. `repos` starts empty.
//! - Write with `manifest::save`, then print the path and next steps.

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Result, bail};

use crate::cli::InitArgs;
use crate::commands::Context;
use crate::manifest::{self, Defaults, Identity, Manifest};

/// Root used when `--root` is not given.
const DEFAULT_ROOT: &str = "~/workspace";

/// Run `grm init`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(args: InitArgs, ctx: &Context) -> Result<ExitCode> {
    // RUST: `match` on an `Option`: use the `-f` path if there is one,
    // otherwise compute the global path (which can fail, hence `?`).
    let path = match &ctx.manifest_path {
        Some(p) => p.clone(),
        None => manifest::global_manifest_path()?,
    };

    let identity = global_git_identity();
    let manifest = write_new_manifest(&path, args.root.as_deref(), args.force, identity)?;

    println!("Created {}", path.display());
    println!("  root: {}", manifest.root);
    if let Some(id) = &manifest.defaults.identity {
        // RUST: `as_deref()` turns `&Option<String>` into `Option<&str>`, and
        // `unwrap_or` supplies a fallback for `None` (this is not `unwrap()`:
        // it can't panic).
        println!(
            "  identity: {} <{}> (from your global git config)",
            id.name.as_deref().unwrap_or("?"),
            id.email.as_deref().unwrap_or("?")
        );
    }

    // If the user picked a custom location, later commands need `-f` too
    // (unless it is a `grm.yml` they run grm from/under).
    let file_flag = if ctx.manifest_path.is_some() {
        format!(" -f {}", path.display())
    } else {
        String::new()
    };
    println!();
    println!("Next steps:");
    println!("  grm{file_flag} import --dry-run   # preview existing checkouts under the root");
    println!("  grm{file_flag} import             # add them to the manifest");
    println!("  grm{file_flag} add owner/name     # add (and clone) a repo");
    println!("  grm{file_flag} sync               # clone missing repos, apply config");
    Ok(ExitCode::SUCCESS)
}

/// Build the new manifest and write it to `path`.
///
/// Errors if `path` already exists and `force` is false. Returns the
/// manifest that was written (handy for printing and for tests).
fn write_new_manifest(
    path: &Path,
    root: Option<&str>,
    force: bool,
    identity: Option<Identity>,
) -> Result<Manifest> {
    if path.exists() && !force {
        bail!(
            "{} already exists (use --force to overwrite it)",
            path.display()
        );
    }
    let manifest = Manifest {
        root: root.unwrap_or(DEFAULT_ROOT).to_string(),
        defaults: Defaults {
            identity,
            // RUST: `..Defaults::default()` ("struct update syntax") fills
            // every field we didn't list from another value of the same type.
            ..Defaults::default()
        },
        repos: Vec::new(),
    };
    // Catch a relative `--root` now rather than on the first `grm sync`.
    manifest.root_dir()?;
    manifest::save(&manifest, path)?;
    Ok(manifest)
}

/// `user.name` / `user.email` from the *global* git config, as an identity.
/// Returns `None` if neither is set (or git can't be run).
fn global_git_identity() -> Option<Identity> {
    let name = global_git_config("user.name");
    let email = global_git_config("user.email");
    if name.is_none() && email.is_none() {
        return None;
    }
    Some(Identity {
        name,
        email,
        signing: None,
    })
}

/// `git config --global --get <key>`, or `None` if unset / git failed.
///
/// `git.rs` only touches `--local` config on purpose, so this one read-only
/// global lookup lives here.
fn global_git_config(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--global", "--get", key])
        .stdin(Stdio::null())
        .output()
        // RUST: `.ok()?` – turn the `Result` into an `Option` and return
        // `None` early from the function if it was an error.
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // RUST: `bool::then_some(x)` gives `Some(x)` when true, else `None`.
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_defaults_and_identity() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("sub/grm.yml");
        let id = Identity {
            name: Some("Jane".into()),
            email: Some("jane@example.com".into()),
            signing: None,
        };
        write_new_manifest(&path, None, false, Some(id.clone())).unwrap();

        let loaded = manifest::load(&path).unwrap();
        assert_eq!(loaded.root, "~/workspace");
        assert_eq!(loaded.defaults.host, "github.com");
        assert_eq!(loaded.defaults.protocol, crate::spec::Protocol::Ssh);
        assert_eq!(loaded.defaults.identity, Some(id));
        assert!(loaded.repos.is_empty());
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("grm.yml");
        write_new_manifest(&path, Some("/tmp/a"), false, None).unwrap();

        let err = write_new_manifest(&path, Some("/tmp/b"), false, None).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(manifest::load(&path).unwrap().root, "/tmp/a");

        write_new_manifest(&path, Some("/tmp/b"), true, None).unwrap();
        assert_eq!(manifest::load(&path).unwrap().root, "/tmp/b");
    }

    #[test]
    fn rejects_relative_root() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("grm.yml");
        assert!(write_new_manifest(&path, Some("relative/dir"), false, None).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn run_uses_manifest_path_from_context() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("grm.yml");
        let ctx = Context {
            manifest_path: Some(path.clone()),
        };
        let args = InitArgs {
            root: Some(tmp.path().display().to_string()),
            force: false,
        };
        assert_eq!(run(args, &ctx).unwrap(), ExitCode::SUCCESS);
        assert!(path.is_file());
    }
}
