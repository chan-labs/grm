//! Shared helpers for the end-to-end CLI tests in `tests/cli.rs`.
//!
//! Each test builds a [`Fixture`]: a temp directory holding
//!
//! - `remotes/<name>.git` – bare repos with one commit on `main`, used as
//!   `origin` remotes via `file://` URLs (no network);
//! - `root/` – the manifest `root`, where grm clones repos;
//! - `home/` – a fake `HOME` / `XDG_CONFIG_HOME`, so the real home directory
//!   is never touched;
//! - `gitconfig` – an empty global git config (`GIT_CONFIG_GLOBAL`);
//! - `grm.yml` – the manifest (written with [`Fixture::write_manifest`]).

// RUST: each file in `tests/` is compiled as its own crate and may use only
// some of these helpers, so silence "never used" warnings here.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// An isolated sandbox for one test. Dropping it deletes everything.
pub struct Fixture {
    // RUST: kept only so the directory lives as long as the fixture. `TempDir`
    // implements `Drop` (Rust's destructor): when the fixture goes out of
    // scope at the end of a test, the whole temp directory is deleted.
    _dir: TempDir,
    /// The temp directory itself.
    pub base: PathBuf,
    /// Where the bare "remote" repos live (outside `root`).
    pub remotes: PathBuf,
    /// The manifest `root`.
    pub root: PathBuf,
    /// Fake home directory.
    pub home: PathBuf,
    /// Empty global git config.
    pub gitconfig: PathBuf,
    /// Manifest path passed with `--file`.
    pub manifest: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("grm-test-")
            .tempdir()
            .expect("create tempdir");
        // Canonicalize so paths compare equal to what git / grm report
        // (e.g. on macOS `/var` is a symlink to `/private/var`).
        let base = dir.path().canonicalize().expect("canonicalize tempdir");
        let remotes = base.join("remotes");
        let root = base.join("root");
        let home = base.join("home");
        let gitconfig = base.join("gitconfig");
        for d in [&remotes, &root, &home] {
            fs::create_dir_all(d).expect("create fixture dir");
        }
        fs::write(&gitconfig, "").expect("write empty gitconfig");
        let manifest = base.join("grm.yml");
        Fixture {
            _dir: dir,
            base,
            remotes,
            root,
            home,
            gitconfig,
            manifest,
        }
    }

    /// A `git` command with the sandboxed environment (no user/system config).
    pub fn git_cmd(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&self.base);
        cmd
    }

    /// Run git with `args` in `dir`, panicking on failure; returns trimmed stdout.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self
            .git_cmd()
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} in {} failed: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Commit everything in `dir` (with a throwaway identity passed via `-c`,
    /// so the repo's local config is left untouched).
    pub fn commit_all(&self, dir: &Path, msg: &str) {
        self.git(dir, &["add", "-A"]);
        self.git(
            dir,
            &[
                "-c",
                "user.name=Test Setup",
                "-c",
                "user.email=setup@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                msg,
            ],
        );
    }

    /// Create `remotes/<name>.git`: a bare repo with one commit on `main`.
    /// Returns its `file://` URL.
    pub fn bare_repo(&self, name: &str) -> String {
        let bare = self.remotes.join(format!("{name}.git"));
        let seed = self.base.join("seed").join(name);
        fs::create_dir_all(&seed).expect("create seed dir");
        self.git(
            &seed,
            &["-c", "init.defaultBranch=main", "init", "-q", "-b", "main"],
        );
        self.git(&seed, &["config", "user.name", "Seed"]);
        self.git(&seed, &["config", "user.email", "seed@example.com"]);
        fs::write(seed.join("README.md"), format!("# {name}\n")).expect("write README");
        self.commit_all(&seed, "initial commit");
        self.git(
            &self.base,
            &[
                "-c",
                "init.defaultBranch=main",
                "clone",
                "-q",
                "--bare",
                seed.to_str().expect("utf-8 path"),
                bare.to_str().expect("utf-8 path"),
            ],
        );
        self.url(name)
    }

    /// `file://` URL of `remotes/<name>.git`.
    pub fn url(&self, name: &str) -> String {
        format!(
            "file://{}",
            self.remotes.join(format!("{name}.git")).display()
        )
    }

    /// Default checkout path for a repo named `name` (flat layout `{root}/{name}`).
    pub fn repo_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Clone `url` to `dest` with plain git (bypassing grm), e.g. to create
    /// an "extra" repo under root. Sets `main` to track `origin/main`.
    pub fn clone_to(&self, url: &str, dest: &Path) {
        self.git(
            &self.base,
            &["clone", "-q", url, dest.to_str().expect("utf-8 path")],
        );
    }

    /// Write the manifest: `root: <abs root>` followed by `body` (YAML with
    /// `defaults:` / `repos:` etc.).
    pub fn write_manifest(&self, body: &str) {
        let yaml = format!("root: {}\n{body}", self.root.display());
        fs::write(&self.manifest, yaml).expect("write manifest");
    }

    /// Manifest with the given repo URLs (short form) and a default identity.
    pub fn write_simple_manifest(&self, urls: &[&str]) {
        let mut body = String::from(
            "defaults:\n  identity:\n    name: Default Name\n    email: default@example.com\nrepos:\n",
        );
        for url in urls {
            body.push_str(&format!("  - {url}\n"));
        }
        self.write_manifest(&body);
    }

    /// Current manifest contents.
    pub fn read_manifest(&self) -> String {
        fs::read_to_string(&self.manifest).expect("read manifest")
    }

    /// `git config --local --get <key>` in `repo`; `None` when unset.
    pub fn config(&self, repo: &Path, key: &str) -> Option<String> {
        let out = self
            .git_cmd()
            .arg("-C")
            .arg(repo)
            .args(["config", "--local", "--get", key])
            .output()
            .expect("run git config");
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// `origin` URL of `repo`, or `None` if there is no origin.
    pub fn origin(&self, repo: &Path) -> Option<String> {
        self.config(repo, "remote.origin.url")
    }

    /// The `grm` binary with `--file <manifest>` and a sandboxed environment.
    pub fn grm(&self) -> assert_cmd::Command {
        self.grm_with_file(&self.manifest)
    }

    /// Like [`Fixture::grm`] but passing `--file <manifest>` instead of the
    /// fixture's default manifest path.
    pub fn grm_with_file(&self, manifest: &Path) -> assert_cmd::Command {
        // RUST: `cargo_bin_cmd!` is a macro that finds the `grm` binary cargo
        // built for this test run (via the `CARGO_BIN_EXE_grm` env var).
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("grm");
        cmd.arg("--file")
            .arg(manifest)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("NO_COLOR", "1")
            .env_remove("GRM_FILE")
            .current_dir(&self.base)
            // Never block on a confirmation prompt.
            .write_stdin("");
        cmd
    }
}
