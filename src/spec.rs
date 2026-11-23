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
//! (e.g. GitLab `group/sub/name`) are supported by putting everything
//! between host and last segment into `owner`.
//!
//! Local repositories (handy for tests with bare repos in a temp dir) are
//! accepted too: `file:///abs/path/name(.git)` or an absolute path
//! `/abs/path/name(.git)`. They get host `local`, owner = parent directory
//! name, name = last path component, and `url` is kept exactly as given.

// RUST: `#![...]` (with `!`) is an "inner attribute" applying to this whole
// file. Nothing outside tests calls this module yet (the commands are still
// stubs), so silence "never used" warnings until they do.
// TODO: remove once the commands use `parse` / `normalize_url`.

use anyhow::{Result, bail};
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

/// Host name given to local (`file://` or absolute path) repos.
const LOCAL_HOST: &str = "local";

/// The pieces pulled out of an explicit URL (as opposed to shorthand).
#[derive(Debug)]
struct UrlParts {
    host: String,
    owner: String,
    name: String,
    /// The protocol-agnostic comparison key (see [`normalize_url`]).
    canonical: String,
}

/// Parse user input into a [`RepoSpec`], filling gaps from `defaults`.
///
/// Errors (with a helpful message) on empty input, a bare name with no owner,
/// or URLs that don't contain an owner and name.
pub fn parse(input: &str, defaults: &Defaults) -> Result<RepoSpec> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty repo spec: expected `owner/name` or a git URL");
    }

    // RUST: `if let Some(x) = expr { ... }` runs the block only when `expr`
    // is `Some`, binding the inner value to `x`. `parse_explicit` returns
    // `None` for shorthand input and `Some(result)` for URLs / local paths.
    if let Some(parts) = parse_explicit(input) {
        // RUST: `?` returns early with the error if the `Result` is `Err`,
        // otherwise it evaluates to the value inside `Ok`.
        let parts = parts?;
        return Ok(RepoSpec {
            host: parts.host,
            owner: parts.owner,
            name: parts.name,
            url: input.to_string(),
        });
    }

    // Shorthand: `owner/name` or `host/owner/name` (owner may be nested).
    let path = strip_repo_suffix(input);
    // RUST: `.split('/')` yields an iterator over the pieces; `.collect()`
    // gathers them into a collection, here a `Vec<&str>` (a growable list of
    // borrowed string slices).
    let segments: Vec<&str> = path.split('/').collect();
    let (host, owner, name) = match segments.as_slice() {
        // RUST: slice patterns: `[_]` matches exactly one element, `[a, b]`
        // exactly two, and `[first, middle @ .., last]` binds the first and
        // last elements plus everything in between as a sub-slice.
        [] | [_] => bail!(
            "invalid repo spec `{input}`: missing owner (expected `owner/name`, \
             `host/owner/name` or a git URL)"
        ),
        [owner, name] => (defaults.host.to_ascii_lowercase(), owner.to_string(), *name),
        [host, middle @ .., name] => {
            validate_host(host, input)?;
            (host.to_ascii_lowercase(), middle.join("/"), *name)
        }
    };
    validate_owner_name(&owner, name, input)?;

    let url = match defaults.protocol {
        Protocol::Ssh => format!("git@{host}:{owner}/{name}.git"),
        Protocol::Https => format!("https://{host}/{owner}/{name}.git"),
    };
    Ok(RepoSpec {
        host,
        owner,
        name: name.to_string(),
        url,
    })
}

/// Normalize a git URL to a canonical, protocol-agnostic comparable form.
///
/// Used to decide whether two URLs point at the same repo (e.g. manifest URL
/// vs. a checkout's `origin`). Output is `host/owner/name`, all lowercase,
/// with no scheme, user (`git@`), port, `.git` suffix or trailing slash. So
/// `git@GitHub.com:Foo/Bar.git`, `ssh://git@github.com/foo/bar` and
/// `https://github.com/foo/bar/` all normalize to `github.com/foo/bar`.
///
/// Local repos (`file://` URLs or absolute paths) normalize to
/// `local/<full/path>` so different directories never compare equal.
///
/// Infallible: if the input can't be understood, return it trimmed and
/// lowercased so comparison still works.
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    // RUST: `match` on an `Option<Result<..>>` handles all three cases
    // (`None`, `Some(Ok(..))`, `Some(Err(..))`) in one place.
    match parse_explicit(trimmed) {
        Some(Ok(parts)) => parts.canonical,
        Some(Err(_)) => trimmed.to_lowercase(),
        // Shorthand like `github.com/foo/bar.git`: just tidy it up.
        None => strip_repo_suffix(trimmed).to_lowercase(),
    }
}

/// Parse explicit URLs and local paths.
///
/// Returns `None` when `input` looks like shorthand (`owner/name`,
/// `host/owner/name`), so the caller can handle it with defaults.
fn parse_explicit(input: &str) -> Option<Result<UrlParts>> {
    // Local repos: `file:///path` or `/path`.
    if let Some(rest) = input.strip_prefix("file://") {
        return Some(parse_local(rest, input));
    }
    if input.starts_with('/') {
        return Some(parse_local(input, input));
    }

    // `scheme://[user@]host[:port]/owner/name`
    if let Some((scheme, rest)) = input.split_once("://") {
        return Some(parse_scheme_url(scheme, rest, input));
    }

    // scp-style `[user@]host:owner/name`. The part before the first `:` must
    // not contain `/` (otherwise it's a path, not a host).
    if let Some((before, path)) = input.split_once(':')
        && !before.contains('/')
    {
        // RUST: `rsplit_once` splits at the *last* occurrence; `map_or`
        // turns an `Option` into a value, using the default when `None`.
        let host = before.rsplit_once('@').map_or(before, |(_, h)| h);
        return Some(remote_parts(host, path, input));
    }

    None
}

/// Handle `scheme://authority/path` URLs.
fn parse_scheme_url(scheme: &str, rest: &str, input: &str) -> Result<UrlParts> {
    let scheme = scheme.to_ascii_lowercase();
    // RUST: `matches!` is a macro returning `true` if the value fits the
    // pattern – a compact alternative to a `match` with `true`/`false` arms.
    if !matches!(scheme.as_str(), "ssh" | "https" | "http" | "git") {
        bail!("unsupported URL scheme `{scheme}` in `{input}` (use ssh, https or file)");
    }
    // RUST: `let PATTERN = expr else { ... }` destructures, or, if the
    // pattern doesn't match, runs the `else` block (which must exit).
    let Some((authority, path)) = rest.split_once('/') else {
        bail!("invalid repo URL `{input}`: missing `/owner/name` path");
    };
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host_port.split_once(':').map_or(host_port, |(h, _)| h);
    remote_parts(host, path, input)
}

/// Build [`UrlParts`] for a remote host and an `owner/name` path.
fn remote_parts(host: &str, path: &str, input: &str) -> Result<UrlParts> {
    validate_host(host, input)?;
    let path = strip_repo_suffix(path.trim_start_matches('/'));
    let Some((owner, name)) = path.rsplit_once('/') else {
        bail!("invalid repo URL `{input}`: expected `owner/name` after the host");
    };
    validate_owner_name(owner, name, input)?;
    let host = host.to_ascii_lowercase();
    let canonical = format!("{host}/{owner}/{name}").to_lowercase();
    Ok(UrlParts {
        host,
        owner: owner.to_string(),
        name: name.to_string(),
        canonical,
    })
}

/// Build [`UrlParts`] for a local repo at absolute path `path`.
fn parse_local(path: &str, input: &str) -> Result<UrlParts> {
    if !path.starts_with('/') {
        bail!("invalid local repo `{input}`: path must be absolute");
    }
    let path = strip_repo_suffix(path);
    let Some((parent, name)) = path.rsplit_once('/') else {
        bail!("invalid local repo `{input}`");
    };
    let owner = parent.rsplit('/').next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        bail!("invalid local repo `{input}`: expected `/<dir>/<owner>/<name>`");
    }
    validate_owner_name(owner, name, input)?;
    Ok(UrlParts {
        host: LOCAL_HOST.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
        canonical: format!("{LOCAL_HOST}{path}").to_lowercase(),
    })
}

/// Strip trailing `/` characters and then a trailing `.git`.
// RUST: the returned `&str` borrows from `s` (it's a sub-slice of it), so no
// new string is allocated. The compiler infers that the output lives as long
// as the input ("lifetime elision").
fn strip_repo_suffix(s: &str) -> &str {
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.trim_end_matches('/')
}

fn validate_host(host: &str, input: &str) -> Result<()> {
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c == '/') {
        bail!("invalid repo spec `{input}`: bad host `{host}`");
    }
    Ok(())
}

fn validate_owner_name(owner: &str, name: &str, input: &str) -> Result<()> {
    // RUST: `|seg| ...` is a closure (an inline anonymous function). `.any`
    // calls it for each item and returns `true` as soon as one returns `true`.
    let bad_segment = |seg: &str| {
        seg.is_empty() || seg == "." || seg == ".." || seg.contains(char::is_whitespace)
    };
    if owner.split('/').any(bad_segment) {
        bail!("invalid repo spec `{input}`: bad owner `{owner}`");
    }
    if bad_segment(name) {
        bail!("invalid repo spec `{input}`: bad repo name `{name}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults(protocol: Protocol) -> Defaults {
        Defaults {
            host: "github.com".to_string(),
            protocol,
            identity: None,
        }
    }

    fn ssh() -> Defaults {
        defaults(Protocol::Ssh)
    }

    fn spec(host: &str, owner: &str, name: &str, url: &str) -> RepoSpec {
        RepoSpec {
            host: host.into(),
            owner: owner.into(),
            name: name.into(),
            url: url.into(),
        }
    }

    #[test]
    fn shorthand_ssh() {
        assert_eq!(
            parse("rust-lang/cargo", &ssh()).unwrap(),
            spec(
                "github.com",
                "rust-lang",
                "cargo",
                "git@github.com:rust-lang/cargo.git"
            )
        );
    }

    #[test]
    fn shorthand_https() {
        assert_eq!(
            parse("rust-lang/cargo", &defaults(Protocol::Https)).unwrap(),
            spec(
                "github.com",
                "rust-lang",
                "cargo",
                "https://github.com/rust-lang/cargo.git"
            )
        );
    }

    #[test]
    fn shorthand_uses_default_host() {
        let d = Defaults {
            host: "gitlab.example.com".into(),
            ..ssh()
        };
        let s = parse("me/tool", &d).unwrap();
        assert_eq!(s.host, "gitlab.example.com");
        assert_eq!(s.url, "git@gitlab.example.com:me/tool.git");
    }

    #[test]
    fn shorthand_strips_git_and_slash() {
        let s = parse("  foo/bar.git/ ", &ssh()).unwrap();
        assert_eq!((s.owner.as_str(), s.name.as_str()), ("foo", "bar"));
        assert_eq!(s.url, "git@github.com:foo/bar.git");
    }

    #[test]
    fn shorthand_with_host_and_nested_groups() {
        let s = parse("gitlab.com/me/tool", &ssh()).unwrap();
        assert_eq!(
            s,
            spec("gitlab.com", "me", "tool", "git@gitlab.com:me/tool.git")
        );

        let s = parse("gitlab.com/group/sub/name", &defaults(Protocol::Https)).unwrap();
        assert_eq!(
            s,
            spec(
                "gitlab.com",
                "group/sub",
                "name",
                "https://gitlab.com/group/sub/name.git"
            )
        );
    }

    #[test]
    fn scp_style() {
        let input = "git@github.com:foo/bar.git";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("github.com", "foo", "bar", input)
        );
        let input = "git@github.com:foo/bar";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("github.com", "foo", "bar", input)
        );
    }

    #[test]
    fn ssh_url_with_and_without_port() {
        let input = "ssh://git@github.com/foo/bar.git";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("github.com", "foo", "bar", input)
        );
        let input = "ssh://git@git.example.com:2222/team/sub/repo/";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("git.example.com", "team/sub", "repo", input)
        );
    }

    #[test]
    fn https_url() {
        let input = "https://github.com/foo/bar.git";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("github.com", "foo", "bar", input)
        );
        let input = "https://github.com/foo/bar/";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("github.com", "foo", "bar", input)
        );
    }

    #[test]
    fn explicit_url_host_is_lowercased_but_url_kept() {
        let input = "https://GitHub.com/Foo/Bar.git";
        let s = parse(input, &ssh()).unwrap();
        assert_eq!(s, spec("github.com", "Foo", "Bar", input));
    }

    #[test]
    fn local_file_url_and_path() {
        let input = "file:///tmp/xyz/remotes/foo.git";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("local", "remotes", "foo", input)
        );
        let input = "/tmp/xyz/remotes/foo";
        assert_eq!(
            parse(input, &ssh()).unwrap(),
            spec("local", "remotes", "foo", input)
        );
    }

    #[test]
    fn junk_is_rejected() {
        for bad in [
            "",
            "   ",
            "justaname",
            "foo/",
            "/foo",
            "/",
            "file://relative/x",
            "https://github.com",
            "https://github.com/onlyowner",
            "ftp://example.com/a/b",
            "git@github.com:onlyname",
            "has space/name",
            "owner/na me",
            "owner/..",
            "host//name",
        ] {
            assert!(parse(bad, &ssh()).is_err(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn error_message_mentions_owner() {
        let err = parse("cargo", &ssh()).unwrap_err().to_string();
        assert!(err.contains("missing owner"), "{err}");
    }

    #[test]
    fn normalize_equates_protocols() {
        for url in [
            "git@GitHub.com:Foo/Bar.git",
            "ssh://git@github.com/foo/bar",
            "ssh://git@github.com:22/foo/bar.git",
            "https://github.com/foo/bar/",
            "https://github.com/foo/bar.git",
            "http://user@github.com/Foo/Bar",
            "github.com/foo/bar",
        ] {
            assert_eq!(normalize_url(url), "github.com/foo/bar", "for {url}");
        }
    }

    #[test]
    fn normalize_local() {
        assert_eq!(normalize_url("file:///tmp/A/foo.git"), "local/tmp/a/foo");
        assert_eq!(normalize_url("/tmp/A/foo/"), "local/tmp/a/foo");
        assert_ne!(normalize_url("/x/a/foo"), normalize_url("/y/a/foo"));
    }

    #[test]
    fn normalize_garbage_is_trimmed_lowercase() {
        assert_eq!(normalize_url("  ftp://Weird  "), "ftp://weird");
    }

    #[test]
    fn shorthand_urls_normalize_equal_across_protocols() {
        let s = parse("Foo/Bar", &ssh()).unwrap();
        let h = parse("Foo/Bar", &defaults(Protocol::Https)).unwrap();
        assert_eq!(normalize_url(&s.url), normalize_url(&h.url));
        assert_eq!(normalize_url(&s.url), "github.com/foo/bar");
    }
}
