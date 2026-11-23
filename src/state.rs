//! Compare the manifest against what is actually on disk ("drift detection").
//!
//! Used by `status` (to report), `sync` (to decide what to fix) and `prune`
//! (via `Report::extras`).
//!
//! STUB: `inspect` is implemented by a later task. Remove the `allow` below
//! once done.

#![allow(dead_code, unused_variables)]

use std::path::PathBuf;

use anyhow::Result;

use crate::manifest::{Manifest, ResolvedRepo};

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
    /// True if reality doesn't match the manifest: any repo is `Missing` or
    /// `Drifted`, or there are `extras`. `status` exits 1 when this is true.
    pub fn has_drift(&self) -> bool {
        // RUST: iterator adapters like `.any(...)` take a closure (`|r| ...`),
        // an inline anonymous function.
        !self.extras.is_empty() || self.repos.iter().any(|r| r.state != RepoState::Ok)
    }
}

/// Resolve the manifest and inspect every repo on disk (read-only; no fetch).
///
/// For each existing repo: compare `identity.to_git_config()` against
/// `git::config_get`, `origin` against `spec.url` (normalized), and add
/// `Dirty` / `AheadBehind` (only when non-zero) informational drift.
pub fn inspect(m: &Manifest) -> Result<Report> {
    todo!("state::inspect")
}
