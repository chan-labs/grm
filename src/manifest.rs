//! The `grm.yml` manifest: its on-disk format, loading/saving, and resolving
//! entries into concrete repos with absolute paths and merged identities.
//!
//! Example manifest:
//!
//! ```yaml
//! root: ~/src
//! defaults:
//!   host: github.com
//!   protocol: ssh
//!   identity:
//!     name: Jane Doe
//!     email: jane@example.com
//!     signing:
//!       key: ~/.ssh/id_ed25519.pub
//!       format: ssh
//! repos:
//!   - rust-lang/cargo
//!   - url: git@gitlab.com:work/api.git
//!     path: work/api
//!     branch: main
//!     identity:
//!       email: jane@work.example
//! ```
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

#![allow(dead_code, unused_variables)]

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::spec::{Protocol, RepoSpec};

/// Default manifest file name.
pub const MANIFEST_FILE: &str = "grm.yml";

/// Top-level manifest document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Directory repos are cloned under. Must be absolute or start with `~`
    /// (relative roots are rejected by `root_dir`).
    pub root: String,
    // RUST: `#[serde(default)]` means "if missing in YAML, use `Default`".
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
}

/// Defaults applied to every repo unless overridden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    /// Host used for `owner/name` shorthand. Default `github.com`.
    #[serde(default = "default_host")]
    pub host: String,
    /// Protocol used to build URLs from shorthand. Default `ssh`.
    #[serde(default)]
    pub protocol: Protocol,
    /// Base identity applied to every repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
}

fn default_host() -> String {
    "github.com".to_string()
}

// RUST: `impl Trait for Type` implements a trait by hand. We can't derive
// `Default` here because `host` must default to "github.com", not "".
impl Default for Defaults {
    fn default() -> Self {
        Self {
            host: default_host(),
            protocol: Protocol::default(),
            identity: None,
        }
    }
}

/// Git identity (author + optional commit signing) for a repo.
///
/// Every field is optional so a repo can override just one part of the
/// default identity.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Identity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<Signing>,
}

/// Commit/tag signing configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signing {
    /// Key ID (OpenPGP) or path to public key (SSH). May start with `~`.
    pub key: String,
    /// Defaults to `ssh` when omitted.
    #[serde(default)]
    pub format: SigningFormat,
}

/// Value of git's `gpg.format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigningFormat {
    #[default]
    Ssh,
    Openpgp,
}

/// One entry in `repos:` — either a bare spec string or a full mapping.
// RUST: `untagged` tells serde to try each variant in order: a YAML string
// becomes `Short`, a YAML mapping becomes `Full`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepoEntry {
    /// Any spec accepted by `spec::parse`, e.g. `owner/name`.
    Short(String),
    Full {
        /// Any spec accepted by `spec::parse` (not only full URLs).
        url: String,
        /// Checkout path: relative to root, or absolute / `~`. Default
        /// `{root}/{host}/{owner}/{name}`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// Branch to check out on clone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// Overrides merged over `defaults.identity`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<Identity>,
    },
}

/// A manifest entry resolved against defaults: everything needed to act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRepo {
    pub spec: RepoSpec,
    /// Absolute checkout path.
    pub path: PathBuf,
    pub branch: Option<String>,
    /// Entry identity merged over `defaults.identity` (may be all-`None`).
    pub identity: Identity,
}

/// Find the manifest to use.
///
/// Order: `cli_file` if given (from `-f` / `GRM_FILE`; returned as-is, even if
/// it doesn't exist yet, so `init` can create it) → `grm.yml` in the current
/// directory or any ancestor → `~/.config/grm/grm.yml` (via `dirs::config_dir`)
/// if it exists. Errors with a hint to run `grm init` if nothing is found.
pub fn discover(cli_file: Option<&Path>) -> Result<PathBuf> {
    todo!("manifest::discover")
}

/// Read and parse a manifest file. Errors include the path as context.
pub fn load(path: &Path) -> Result<Manifest> {
    todo!("manifest::load")
}

/// Serialize `manifest` as YAML and write it to `path` (atomically: write a
/// temp file in the same dir, then rename). Creates parent dirs if needed.
pub fn save(manifest: &Manifest, path: &Path) -> Result<()> {
    todo!("manifest::save")
}

/// Expand a leading `~` / `~/` to the home directory (`dirs::home_dir`).
/// Other paths are returned unchanged. Errors if home can't be determined.
pub fn expand_tilde(path: &str) -> Result<PathBuf> {
    todo!("manifest::expand_tilde")
}

impl Manifest {
    /// `root` with `~` expanded. Errors if the result is not absolute.
    pub fn root_dir(&self) -> Result<PathBuf> {
        todo!("Manifest::root_dir")
    }

    /// Resolve every entry: parse its spec with `spec::parse(_, &self.defaults)`,
    /// compute its absolute path (default `root_dir()/host/owner/name`; a
    /// relative `path` is joined to `root_dir()`; `~` is expanded), and merge
    /// its identity over `defaults.identity`.
    ///
    /// Errors (naming the offending entry) on unparseable specs or on two
    /// entries resolving to the same path or the same normalized URL.
    pub fn resolve(&self) -> Result<Vec<ResolvedRepo>> {
        todo!("Manifest::resolve")
    }
}

impl Identity {
    /// Field-by-field merge: each `Some` field of `self` wins, otherwise the
    /// value from `base` is used. `signing` is replaced as a whole (not
    /// merged key/format).
    pub fn merged_over(&self, base: &Identity) -> Identity {
        todo!("Identity::merged_over")
    }

    /// The local git config this identity implies, as `(key, value)` pairs.
    /// `None` means the key should be *unset* in the repo's local config.
    ///
    /// Always returns the same keys in this order:
    /// - `user.name`        ← `name`
    /// - `user.email`       ← `email`
    /// - `user.signingkey`  ← `signing.key` (`~` expanded for SSH format)
    /// - `gpg.format`       ← `ssh` / `openpgp`
    /// - `commit.gpgsign`   ← `true` if signing, else `None`
    /// - `tag.gpgsign`      ← `true` if signing, else `None`
    pub fn to_git_config(&self) -> Vec<(String, Option<String>)> {
        todo!("Identity::to_git_config")
    }
}
