//! Thin wrappers around the `git` command-line tool.
//!
//! Every function shells out to `git` (via `std::process::Command`, using
//! `git -C <repo> ...` for repo-scoped calls) and returns `anyhow::Result`.
//! Non-zero exit codes become errors that include git's stderr, except where
//! documented (e.g. `config_get` on a missing key returns `Ok(None)`).
//! Config reads/writes always use `--local`, so global config is never touched.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(dead_code, unused_variables)]

use std::path::{Path, PathBuf};

use anyhow::Result;

/// `git clone [--branch <branch>] <url> <dest>`. Creates parent dirs of
/// `dest`. Inherits stdin/stderr so SSH prompts and progress are visible.
pub fn clone(url: &str, dest: &Path, branch: Option<&str>) -> Result<()> {
    todo!("git::clone")
}

/// `git config --local --get <key>`. Exit code 1 (key not set) → `Ok(None)`.
pub fn config_get(repo: &Path, key: &str) -> Result<Option<String>> {
    todo!("git::config_get")
}

/// `git config --local <key> <value>`.
pub fn config_set(repo: &Path, key: &str, value: &str) -> Result<()> {
    todo!("git::config_set")
}

/// `git config --local --unset <key>`. Already-unset (exit code 5) is `Ok`.
pub fn config_unset(repo: &Path, key: &str) -> Result<()> {
    todo!("git::config_unset")
}

/// URL of the `origin` remote, or `None` if there is no `origin`.
pub fn remote_url(repo: &Path) -> Result<Option<String>> {
    todo!("git::remote_url")
}

/// Set (or add, if missing) the `origin` remote URL.
pub fn set_remote_url(repo: &Path, url: &str) -> Result<()> {
    todo!("git::set_remote_url")
}

/// True if the working tree has uncommitted changes or untracked files
/// (`git status --porcelain` is non-empty).
pub fn is_dirty(repo: &Path) -> Result<bool> {
    todo!("git::is_dirty")
}

/// True if any local branch has commits not on its upstream, or a local
/// branch has no upstream at all (its commits exist only here).
pub fn has_unpushed(repo: &Path) -> Result<bool> {
    todo!("git::has_unpushed")
}

/// True if `git stash list` is non-empty.
pub fn has_stash(repo: &Path) -> Result<bool> {
    todo!("git::has_stash")
}

/// `(ahead, behind)` counts of HEAD vs. its upstream
/// (`git rev-list --left-right --count HEAD...@{u}`), or `None` if HEAD is
/// detached or has no upstream. Does not fetch.
pub fn ahead_behind(repo: &Path) -> Result<Option<(u32, u32)>> {
    todo!("git::ahead_behind")
}

/// Find git repositories under `root`, descending at most `max_depth`
/// directory levels (`root` itself is depth 0). A directory containing a
/// `.git` entry (dir or file) is a repo; do not descend into repos. Skips
/// symlinks. Returns sorted absolute paths; a missing `root` yields `[]`.
pub fn find_repos(root: &Path, max_depth: usize) -> Result<Vec<PathBuf>> {
    todo!("git::find_repos")
}
