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

// RUST: `mod edit;` inside manifest.rs loads `src/manifest/edit.rs` as a
// child module, `crate::manifest::edit`. `pub use` re-exports `RepoEdit` so
// callers can write `manifest::RepoEdit` without knowing about the submodule.
mod edit;
pub use edit::RepoEdit;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// RUST: `Context as _` imports the trait (so `.context()` works on `Result`)
// without bringing the name `Context` itself into scope.
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

use crate::spec::{self, Protocol, RepoSpec};

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
    // RUST: `skip_serializing_if = "Option::is_none"` leaves the key out of
    // the YAML entirely when it is `None` (instead of writing `identity: null`)
    // so files written by `grm add` / `grm init` stay short.
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
        /// `{root}/{name}` (the repo name, without owner or host).
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

/// The global manifest location: `~/.config/grm/grm.yml` on every platform
/// (deliberately not `dirs::config_dir`, so macOS uses the same path).
pub fn global_manifest_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine the home directory")?;
    Ok(home.join(".config").join("grm").join(MANIFEST_FILE))
}

/// Find the manifest to use.
///
/// Order: `cli_file` if given (from `-f` / `GRM_FILE`; returned as-is, even if
/// it doesn't exist yet, so `init` can create it) → `grm.yml` in the current
/// directory or any ancestor → `~/.config/grm/grm.yml` (see
/// [`global_manifest_path`]) if it exists. Errors with a hint to run
/// `grm init` if nothing is found.
pub fn discover(cli_file: Option<&Path>) -> Result<PathBuf> {
    if let Some(file) = cli_file {
        return Ok(file.to_path_buf());
    }
    let cwd = std::env::current_dir().context("reading the current directory")?;
    // RUST: `.ok()` turns a `Result` into an `Option`, discarding the error:
    // a missing home dir just means "no global manifest to try".
    let global = global_manifest_path().ok();
    discover_from(&cwd, global.as_deref())
}

/// The testable core of [`discover`]: search `cwd` and its ancestors, then
/// `global`.
fn discover_from(cwd: &Path, global: Option<&Path>) -> Result<PathBuf> {
    let mut tried = Vec::new();
    // RUST: `.ancestors()` iterates over `cwd`, its parent, its grandparent,
    // ... up to the filesystem root. A `for` loop consumes any iterator.
    for dir in cwd.ancestors() {
        let candidate = dir.join(MANIFEST_FILE);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    if let Some(global) = global {
        if global.is_file() {
            return Ok(global.to_path_buf());
        }
        tried.push(global.to_path_buf());
    }
    let tried: Vec<String> = tried.iter().map(|p| format!("  {}", p.display())).collect();
    bail!(
        "no {MANIFEST_FILE} found (run `grm init` to create one, or pass --file / set GRM_FILE). Looked in:\n{}",
        tried.join("\n")
    )
}

/// Read and parse a manifest file. Errors include the path as context.
pub fn load(path: &Path) -> Result<Manifest> {
    // RUST: `with_context(|| ...)` only builds the message if there is an
    // error, avoiding the `format!` cost on the happy path.
    let text =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    serde_yaml_ng::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
}

/// Serialize `manifest` as YAML and write it to `path` (atomically: write a
/// temp file in the same dir, then rename). Creates parent dirs if needed.
pub fn save(manifest: &Manifest, path: &Path) -> Result<()> {
    let yaml = serde_yaml_ng::to_string(manifest).context("serializing manifest")?;
    write_atomic(path, &yaml)
}

/// Save `manifest`, which is the file at `path` with `edit` applied to its
/// `repos:` list, by editing the file's text. Comments, blank lines and the
/// order of keys are kept.
///
/// The edited text is parsed again and must give exactly `manifest`. If the
/// file has a shape the text editor doesn't handle, or the check fails, the
/// whole file is rewritten with `save` instead (losing its comments) and a
/// warning is printed.
pub fn save_edit(manifest: &Manifest, path: &Path, edit: RepoEdit<'_>) -> Result<()> {
    let original =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    // RUST: `Option::filter` keeps the value only if the closure returns
    // true, so a text edit that changes the meaning becomes `None`.
    let edited = edit::apply(&original, edit).filter(|text| {
        serde_yaml_ng::from_str::<Manifest>(text).is_ok_and(|parsed| parsed == *manifest)
    });
    match edited {
        Some(text) => write_atomic(path, &text),
        None => {
            eprintln!(
                "warning: couldn't edit {} in place, so it was rewritten and its comments were removed",
                path.display()
            );
            save(manifest, path)
        }
    }
}

/// Write `text` to `path` atomically: write a temp file in the same dir, then
/// rename it over `path`. Creates parent dirs if needed.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    // `Path::new("grm.yml").parent()` is `Some("")`, meaning "current dir".
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    fs::create_dir_all(&dir).with_context(|| format!("creating directory {}", dir.display()))?;

    let file_name = path
        .file_name()
        .with_context(|| format!("manifest path {} has no file name", path.display()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = dir.join(tmp_name);

    fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    // Renaming within one directory is atomic: readers see either the old
    // file or the new one, never a half-written file.
    if let Err(err) = fs::rename(&tmp, path) {
        // Best-effort cleanup; the rename error is the one worth reporting.
        let _ = fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("replacing manifest {}", path.display()));
    }
    Ok(())
}

/// Expand a leading `~` / `~/` to the home directory (`dirs::home_dir`).
/// Other paths are returned unchanged. Errors if home can't be determined.
pub fn expand_tilde(path: &str) -> Result<PathBuf> {
    let home = || dirs::home_dir().context("could not determine the home directory");
    if path == "~" {
        return home();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home()?.join(rest));
    }
    Ok(PathBuf::from(path))
}

// RUST: an `impl Type { ... }` block defines methods on a struct. A method
// whose first parameter is `&self` borrows the value it is called on
// (`manifest.root_dir()`), so the caller keeps ownership of it.
impl Manifest {
    /// `root` with `~` expanded. Errors if the result is not absolute.
    pub fn root_dir(&self) -> Result<PathBuf> {
        let root =
            expand_tilde(&self.root).with_context(|| format!("expanding root `{}`", self.root))?;
        if !root.is_absolute() {
            bail!(
                "manifest root `{}` must be an absolute path or start with `~`",
                self.root
            );
        }
        Ok(root)
    }

    /// Resolve every entry: parse its spec with `spec::parse(_, &self.defaults)`,
    /// compute its absolute path (default `root_dir()/name`, i.e. the repo
    /// name only; a relative `path` is joined to `root_dir()`; `~` is
    /// expanded), and merge its identity over `defaults.identity`.
    ///
    /// Errors (naming the offending entry) on unparseable specs or on two
    /// entries resolving to the same path or the same normalized URL.
    pub fn resolve(&self) -> Result<Vec<ResolvedRepo>> {
        let root = self.root_dir()?;
        // RUST: `unwrap_or_default()` gives the inner value, or
        // `Identity::default()` (all `None`) when there is no default identity.
        let base_identity = self.defaults.identity.clone().unwrap_or_default();

        let mut resolved = Vec::with_capacity(self.repos.len());
        // RUST: `HashMap<K, V>` is a hash table; here it remembers which entry
        // claimed each path / URL so we can name both sides of a clash.
        let mut by_path: HashMap<PathBuf, String> = HashMap::new();
        let mut by_url: HashMap<String, String> = HashMap::new();

        for entry in &self.repos {
            // RUST: `match` with destructuring pulls fields out of each enum
            // variant. `&self.repos` iterates by reference, so the fields are
            // references (`&String`, `&Option<...>`) – no copies are made.
            let (spec_str, path, branch, identity) = match entry {
                RepoEntry::Short(s) => (s, None, None, None),
                RepoEntry::Full {
                    url,
                    path,
                    branch,
                    identity,
                } => (url, path.as_ref(), branch.clone(), identity.as_ref()),
            };

            let spec = spec::parse(spec_str, &self.defaults)
                .with_context(|| format!("in manifest entry `{spec_str}`"))?;

            let path = match path {
                Some(p) => {
                    let expanded = expand_tilde(p)
                        .with_context(|| format!("in manifest entry `{spec_str}`"))?;
                    // `join` with an absolute path replaces the base entirely.
                    root.join(expanded)
                }
                None => root.join(&spec.name),
            };

            if let Some(other) = by_path.insert(path.clone(), spec_str.clone()) {
                bail!(
                    "manifest entries `{other}` and `{spec_str}` both resolve to path {}",
                    path.display()
                );
            }
            let key = spec::normalize_url(&spec.url);
            if let Some(other) = by_url.insert(key.clone(), spec_str.clone()) {
                bail!("manifest entries `{other}` and `{spec_str}` are the same repo ({key})");
            }

            let identity = match identity {
                Some(id) => id.merged_over(&base_identity),
                None => base_identity.clone(),
            };

            resolved.push(ResolvedRepo {
                spec,
                path,
                branch,
                identity,
            });
        }
        Ok(resolved)
    }
}

impl Identity {
    /// Field-by-field merge: each `Some` field of `self` wins, otherwise the
    /// value from `base` is used. `signing` is replaced as a whole (not
    /// merged key/format).
    pub fn merged_over(&self, base: &Identity) -> Identity {
        // RUST: `a.clone().or_else(|| b.clone())` keeps `a` if it is `Some`,
        // otherwise falls back to `b`.
        Identity {
            name: self.name.clone().or_else(|| base.name.clone()),
            email: self.email.clone().or_else(|| base.email.clone()),
            signing: self.signing.clone().or_else(|| base.signing.clone()),
        }
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
        let signing = self.signing.as_ref();
        let signing_key = signing.map(|s| match s.format {
            // If home can't be found, fall back to the key as written.
            SigningFormat::Ssh => expand_tilde(&s.key)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| s.key.clone()),
            SigningFormat::Openpgp => s.key.clone(),
        });
        let format = signing.map(|s| {
            match s.format {
                SigningFormat::Ssh => "ssh",
                SigningFormat::Openpgp => "openpgp",
            }
            .to_string()
        });
        let gpgsign = signing.map(|_| "true".to_string());

        vec![
            ("user.name".to_string(), self.name.clone()),
            ("user.email".to_string(), self.email.clone()),
            ("user.signingkey".to_string(), signing_key),
            ("gpg.format".to_string(), format),
            ("commit.gpgsign".to_string(), gpgsign.clone()),
            ("tag.gpgsign".to_string(), gpgsign),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        dirs::home_dir().unwrap()
    }

    fn manifest(yaml: &str) -> Manifest {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    fn id(name: Option<&str>, email: Option<&str>, signing: Option<Signing>) -> Identity {
        Identity {
            name: name.map(String::from),
            email: email.map(String::from),
            signing,
        }
    }

    #[test]
    fn parses_example_manifest() {
        let m = manifest(
            r#"
root: ~/src
defaults:
  host: github.com
  protocol: https
  identity:
    name: Jane Doe
    email: jane@example.com
    signing:
      key: ~/.ssh/id_ed25519.pub
repos:
  - rust-lang/cargo
  - url: git@gitlab.com:work/api.git
    path: work/api
    branch: main
    identity:
      email: jane@work.example
"#,
        );
        assert_eq!(m.defaults.protocol, Protocol::Https);
        let sig = m
            .defaults
            .identity
            .as_ref()
            .unwrap()
            .signing
            .as_ref()
            .unwrap();
        assert_eq!(sig.format, SigningFormat::Ssh);
        assert_eq!(m.repos[0], RepoEntry::Short("rust-lang/cargo".into()));
        assert!(matches!(&m.repos[1], RepoEntry::Full { path: Some(p), .. } if p == "work/api"));
    }

    #[test]
    fn example_file_parses_and_resolves() {
        // RUST: `include_str!` reads a file at compile time into a `&str`,
        // relative to this source file. If grm.example.yml stops matching the
        // format, this test fails instead of a user's first run.
        let m = manifest(include_str!("../grm.example.yml"));
        let repos = m.resolve().unwrap();
        assert_eq!(repos.len(), m.repos.len());
        assert!(repos.iter().any(|r| r.path == home().join(".dotfiles")));
    }

    #[test]
    fn save_edit_keeps_comments_and_falls_back_when_it_cannot() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("grm.yml");
        fs::write(&path, "# mine\nroot: /r\nrepos:\n  - a/b  # first\n").unwrap();
        let mut m = load(&path).unwrap();
        m.repos.push(RepoEntry::Short("c/d".into()));
        save_edit(&m, &path, RepoEdit::Append(&m.repos[1..])).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# mine\nroot: /r\nrepos:\n  - a/b  # first\n  - c/d\n"
        );

        // A flow-style list isn't edited in place, but the save still works.
        fs::write(&path, "# mine\nroot: /r\nrepos: [a/b]\n").unwrap();
        save_edit(&m, &path, RepoEdit::Append(&m.repos[1..])).unwrap();
        assert_eq!(load(&path).unwrap(), m);
    }

    #[test]
    fn defaults_are_filled_in() {
        let m = manifest("root: /src\n");
        assert_eq!(m.defaults, Defaults::default());
        assert_eq!(m.defaults.host, "github.com");
        assert!(m.repos.is_empty());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/dir/grm.yml");
        let m = Manifest {
            root: "~/src".into(),
            defaults: Defaults {
                identity: Some(id(Some("Jane"), None, None)),
                ..Defaults::default()
            },
            repos: vec![
                RepoEntry::Short("a/b".into()),
                RepoEntry::Full {
                    url: "c/d".into(),
                    path: None,
                    branch: Some("dev".into()),
                    identity: None,
                },
            ],
        };
        save(&m, &path).unwrap();
        assert_eq!(load(&path).unwrap(), m);
        // No temp files left behind.
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1);
        // Overwriting works too.
        save(&Manifest { repos: vec![], ..m }, &path).unwrap();
        assert!(load(&path).unwrap().repos.is_empty());
    }

    #[test]
    fn load_errors_mention_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.yml");
        let err = format!("{:#}", load(&missing).unwrap_err());
        assert!(err.contains("nope.yml"), "{err}");

        let bad = dir.path().join("bad.yml");
        fs::write(&bad, "root: [unclosed").unwrap();
        let err = format!("{:#}", load(&bad).unwrap_err());
        assert!(
            err.contains("parsing manifest") && err.contains("bad.yml"),
            "{err}"
        );
    }

    #[test]
    fn expand_tilde_cases() {
        assert_eq!(expand_tilde("~").unwrap(), home());
        assert_eq!(expand_tilde("~/src/x").unwrap(), home().join("src/x"));
        assert_eq!(expand_tilde("/abs").unwrap(), PathBuf::from("/abs"));
        assert_eq!(expand_tilde("rel/p").unwrap(), PathBuf::from("rel/p"));
        assert_eq!(expand_tilde("~other").unwrap(), PathBuf::from("~other"));
    }

    #[test]
    fn root_dir_rules() {
        assert_eq!(
            manifest("root: ~/src").root_dir().unwrap(),
            home().join("src")
        );
        assert_eq!(
            manifest("root: /src").root_dir().unwrap(),
            PathBuf::from("/src")
        );
        assert!(manifest("root: src").root_dir().is_err());
    }

    #[test]
    fn global_path_is_dot_config() {
        assert_eq!(
            global_manifest_path().unwrap(),
            home().join(".config/grm/grm.yml")
        );
    }

    #[test]
    fn discover_prefers_cli_file() {
        let p = Path::new("/does/not/exist.yml");
        assert_eq!(discover(Some(p)).unwrap(), p);
    }

    #[test]
    fn discover_walks_ancestors_then_global() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        let global = dir.path().join("global/grm.yml");

        // Nothing anywhere: error lists the paths tried.
        let err = discover_from(&deep, Some(&global)).unwrap_err().to_string();
        assert!(err.contains("grm init"), "{err}");
        assert!(
            err.contains(&deep.join("grm.yml").display().to_string()),
            "{err}"
        );
        assert!(err.contains(&global.display().to_string()), "{err}");

        // Global exists.
        fs::create_dir_all(global.parent().unwrap()).unwrap();
        fs::write(&global, "root: /x\n").unwrap();
        assert_eq!(discover_from(&deep, Some(&global)).unwrap(), global);

        // An ancestor's grm.yml beats the global one.
        let ancestor = dir.path().join("a/grm.yml");
        fs::write(&ancestor, "root: /x\n").unwrap();
        assert_eq!(discover_from(&deep, Some(&global)).unwrap(), ancestor);

        // The current dir beats ancestors.
        let here = deep.join("grm.yml");
        fs::write(&here, "root: /x\n").unwrap();
        assert_eq!(discover_from(&deep, Some(&global)).unwrap(), here);
    }

    #[test]
    fn resolve_paths_and_specs() {
        let m = manifest(
            r#"
root: /src
repos:
  - rust-lang/cargo
  - url: https://gitlab.com/work/api.git
    path: work/api
    branch: main
  - url: me/abs
    path: /elsewhere/abs
  - url: me/home
    path: ~/home-repo
"#,
        );
        let r = m.resolve().unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].path, PathBuf::from("/src/cargo"));
        assert_eq!(r[0].spec.url, "git@github.com:rust-lang/cargo.git");
        assert_eq!(r[0].branch, None);
        assert_eq!(r[1].path, PathBuf::from("/src/work/api"));
        assert_eq!(r[1].spec.host, "gitlab.com");
        assert_eq!(r[1].branch.as_deref(), Some("main"));
        assert_eq!(r[2].path, PathBuf::from("/elsewhere/abs"));
        assert_eq!(r[3].path, home().join("home-repo"));
        assert_eq!(r[0].identity, Identity::default());
    }

    #[test]
    fn resolve_rejects_duplicate_paths() {
        let m = manifest("root: /src\nrepos:\n  - alice/tool\n  - bob/tool\n");
        let err = m.resolve().unwrap_err().to_string();
        assert!(
            err.contains("alice/tool") && err.contains("bob/tool"),
            "{err}"
        );
        assert!(err.contains("/src/tool"), "{err}");
    }

    #[test]
    fn resolve_rejects_duplicate_urls() {
        let m = manifest(
            "root: /src\nrepos:\n  - foo/bar\n  - url: https://github.com/Foo/Bar.git\n    path: other\n",
        );
        let err = m.resolve().unwrap_err().to_string();
        assert!(err.contains("same repo"), "{err}");
    }

    #[test]
    fn resolve_names_bad_entry() {
        let m = manifest("root: /src\nrepos:\n  - justaname\n");
        let err = format!("{:#}", m.resolve().unwrap_err());
        assert!(err.contains("justaname"), "{err}");
    }

    #[test]
    fn resolve_merges_identity() {
        let m = manifest(
            r#"
root: /src
defaults:
  identity:
    name: Jane
    email: jane@example.com
repos:
  - a/one
  - url: a/two
    identity:
      email: jane@work.example
"#,
        );
        let r = m.resolve().unwrap();
        assert_eq!(
            r[0].identity,
            id(Some("Jane"), Some("jane@example.com"), None)
        );
        assert_eq!(
            r[1].identity,
            id(Some("Jane"), Some("jane@work.example"), None)
        );
    }

    #[test]
    fn merged_over_replaces_signing_whole() {
        let base = id(
            Some("B"),
            Some("b@x"),
            Some(Signing {
                key: "BASEKEY".into(),
                format: SigningFormat::Openpgp,
            }),
        );
        let over = id(
            None,
            None,
            Some(Signing {
                key: "~/k.pub".into(),
                format: SigningFormat::Ssh,
            }),
        );
        let m = over.merged_over(&base);
        assert_eq!(m.name.as_deref(), Some("B"));
        assert_eq!(m.signing, over.signing);
        assert_eq!(Identity::default().merged_over(&base), base);
    }

    fn cfg(identity: &Identity) -> Vec<(String, Option<String>)> {
        identity.to_git_config()
    }

    fn pairs(v: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        v.iter()
            .map(|(k, v)| (k.to_string(), v.map(String::from)))
            .collect()
    }

    #[test]
    fn git_config_empty_identity_unsets_everything() {
        assert_eq!(
            cfg(&Identity::default()),
            pairs(&[
                ("user.name", None),
                ("user.email", None),
                ("user.signingkey", None),
                ("gpg.format", None),
                ("commit.gpgsign", None),
                ("tag.gpgsign", None),
            ])
        );
    }

    #[test]
    fn git_config_with_ssh_signing() {
        let identity = id(
            Some("Jane"),
            Some("j@x"),
            Some(Signing {
                key: "~/.ssh/id.pub".into(),
                format: SigningFormat::Ssh,
            }),
        );
        let key = home().join(".ssh/id.pub").display().to_string();
        assert_eq!(
            cfg(&identity),
            pairs(&[
                ("user.name", Some("Jane")),
                ("user.email", Some("j@x")),
                ("user.signingkey", Some(&key)),
                ("gpg.format", Some("ssh")),
                ("commit.gpgsign", Some("true")),
                ("tag.gpgsign", Some("true")),
            ])
        );
    }

    #[test]
    fn git_config_with_openpgp_signing_keeps_key() {
        let identity = id(
            None,
            Some("j@x"),
            Some(Signing {
                key: "~ABC123".into(),
                format: SigningFormat::Openpgp,
            }),
        );
        assert_eq!(
            cfg(&identity),
            pairs(&[
                ("user.name", None),
                ("user.email", Some("j@x")),
                ("user.signingkey", Some("~ABC123")),
                ("gpg.format", Some("openpgp")),
                ("commit.gpgsign", Some("true")),
                ("tag.gpgsign", Some("true")),
            ])
        );
    }
}
