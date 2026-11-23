//! Parsing repo "specs" (what the user types) into a canonical [`RepoSpec`].
//!
//! Accepted input forms:
//! - `name`              – not accepted (owner is required); return an error.
//! - `owner/name`        – host + protocol come from [`Defaults`].
//! - `host/owner/name`   – e.g. `gitlab.com/me/tool`; protocol from defaults.
//! - `git@host:owner/name(.git)`           – SSH scp-style URL.
//! - `ssh://git@host(:port)/owner/name(.git)` – SSH URL.
//! - `https://host/owner/name(.git)`       – HTTPS URL.
//!
//! For explicit URLs, `RepoSpec::url` is the URL as given (trimmed). For the
//! shorthand forms, `url` is built from the default protocol:
//! - `Ssh`   → `git@{host}:{owner}/{name}.git`
//! - `Https` → `https://{host}/{owner}/{name}.git`
//!
//! A trailing `.git` and trailing `/` are stripped from `name`. Nested groups
//! (e.g. GitLab `group/sub/name`) may be supported by putting everything
//! between host and last segment into `owner`.
//!
//! STUB: implemented by a later task. Remove the `allow` below once done.

// RUST: `#![...]` (with `!`) is an "inner attribute" applying to this whole
// file. These silence warnings about not-yet-used code in the stub.
#![allow(dead_code, unused_variables)]

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::manifest::Defaults;

/// A fully parsed repository reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoSpec {
    /// e.g. `github.com`
    pub host: String,
    /// e.g. `rust-lang` (may contain `/` for nested groups)
    pub owner: String,
    /// e.g. `cargo` (no `.git` suffix)
    pub name: String,
    /// The clone URL.
    pub url: String,
}

/// Which transport to use when building URLs from shorthand specs.
// RUST: `#[serde(rename_all = "lowercase")]` makes this (de)serialize as
// `ssh` / `https` in YAML. `#[default]` marks the variant `Default` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Ssh,
    Https,
}

/// Parse user input into a [`RepoSpec`], filling gaps from `defaults`.
///
/// Errors (with a helpful message) on empty input, a bare name with no owner,
/// or URLs that don't contain an owner and name.
pub fn parse(input: &str, defaults: &Defaults) -> Result<RepoSpec> {
    todo!("spec::parse")
}

/// Normalize a git URL to a canonical, protocol-agnostic comparable form.
///
/// Used to decide whether two URLs point at the same repo (e.g. manifest URL
/// vs. a checkout's `origin`). Output is `host/owner/name`, all lowercase,
/// with no scheme, user (`git@`), port, `.git` suffix or trailing slash. So
/// `git@GitHub.com:Foo/Bar.git`, `ssh://git@github.com/foo/bar` and
/// `https://github.com/foo/bar/` all normalize to `github.com/foo/bar`.
///
/// Infallible: if the input can't be understood, return it trimmed and
/// lowercased so comparison still works.
pub fn normalize_url(url: &str) -> String {
    todo!("spec::normalize_url")
}
