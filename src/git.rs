//! Thin wrappers around the `git` command-line tool.
//!
//! Every function shells out to `git` (via `std::process::Command`, using
//! `git -C <repo> ...` for repo-scoped calls) and returns `anyhow::Result`.
//! Non-zero exit codes become errors that include git's stderr, except where
//! documented (e.g. `config_get` on a missing key returns `Ok(None)`).
//! Config reads/writes always use `--local`, so global config is never touched.

// The command modules that call these are still stubs, so the compiler sees
// them as unused. Remove this once the commands are implemented.
// RUST: `#![...]` is an attribute applying to the whole module; `allow`
// silences a specific lint (warning).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

// RUST: `anyhow` gives us one flexible error type. `bail!` returns early with
// an error built from a format string; `Context` is a trait that adds the
// `.context(...)` / `.with_context(...)` methods to `Result`s.
use anyhow::{Context, Result, bail};

/// Run `git -C <repo> <args...>` and capture its output, *without* treating a
/// non-zero exit code as an error. Callers that care about specific exit
/// codes (like `config_get`) inspect `output.status` themselves.
fn git_output(repo: &Path, args: &[&str]) -> Result<Output> {
    // RUST: `Command` is a builder: each method call configures it and
    // returns `&mut Command`, so calls can be chained. `.output()` runs the
    // process, waits for it, and collects stdout/stderr into byte vectors.
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        // Never block waiting for a username/password on stdin.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        // RUST: `with_context` takes a closure (`|| ...`), so the message is
        // only built if there actually was an error.
        .with_context(|| format!("failed to run `git {}` (is git installed?)", args.join(" ")))
}

/// Run `git -C <repo> <args...>` and return its trimmed stdout. A non-zero
/// exit code becomes an error that includes the command and git's stderr.
fn git(repo: &Path, args: &[&str]) -> Result<String> {
    // RUST: `?` means "if this is an error, return it from this function
    // right now; otherwise unwrap the success value".
    let output = git_output(repo, args)?;
    if !output.status.success() {
        bail!(
            "`git -C {} {}` failed ({}): {}",
            repo.display(),
            args.join(" "),
            output.status,
            // RUST: git's output is raw bytes; `from_utf8_lossy` turns them
            // into text, replacing any invalid UTF-8 with a placeholder.
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(stdout_string(&output))
}

/// Stdout of a finished command as a trimmed `String`.
fn stdout_string(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// `git clone [--branch <branch>] <url> <dest>`. Creates parent dirs of
/// `dest`. Inherits stdin/stderr so SSH prompts and progress are visible.
pub fn clone(url: &str, dest: &Path, branch: Option<&str>) -> Result<()> {
    // RUST: `if let Some(x) = ...` runs the block only when the `Option`
    // holds a value, binding it to `x`.
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    }
    // `--` stops option parsing, so a URL starting with `-` can't be
    // mistaken for a flag.
    cmd.arg("--").arg(url).arg(dest);
    // HTTPS credential prompts would hang a parallel sync; disable them.
    // (SSH passphrase prompts still work since they use the terminal directly.)
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    // `.status()` (unlike `.output()`) lets the child inherit our
    // stdin/stdout/stderr, so the user sees git's progress output.
    let status = cmd
        .status()
        .with_context(|| format!("failed to run `git clone {url}` (is git installed?)"))?;
    if !status.success() {
        bail!(
            "`git clone {url} {}` failed ({status}); see git's output above",
            dest.display()
        );
    }
    Ok(())
}

/// `git config --local --get <key>`. Exit code 1 (key not set) → `Ok(None)`.
pub fn config_get(repo: &Path, key: &str) -> Result<Option<String>> {
    let args = ["config", "--local", "--get", key];
    let output = git_output(repo, &args)?;
    // RUST: `match` compares a value against patterns; it must cover every
    // case. `status.code()` is an `Option<i32>` (it's `None` if the process
    // was killed by a signal), hence the `Some(...)` patterns.
    match output.status.code() {
        Some(0) => Ok(Some(stdout_string(&output))),
        Some(1) => Ok(None),
        _ => bail!(
            "`git -C {} {}` failed ({}): {}",
            repo.display(),
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

/// `git config --local <key> <value>`.
pub fn config_set(repo: &Path, key: &str, value: &str) -> Result<()> {
    git(repo, &["config", "--local", key, value])?;
    Ok(())
}

/// `git config --local --unset <key>`. Already-unset (exit code 5) is `Ok`.
pub fn config_unset(repo: &Path, key: &str) -> Result<()> {
    let args = ["config", "--local", "--unset", key];
    let output = git_output(repo, &args)?;
    match output.status.code() {
        Some(0) | Some(5) => Ok(()),
        _ => bail!(
            "`git -C {} {}` failed ({}): {}",
            repo.display(),
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

/// URL of the `origin` remote, or `None` if there is no `origin`.
pub fn remote_url(repo: &Path) -> Result<Option<String>> {
    // Reading the config key directly gives a clean "not set" exit code,
    // unlike `git remote get-url`, whose failure modes are harder to tell apart.
    config_get(repo, "remote.origin.url")
}

/// Set (or add, if missing) the `origin` remote URL.
pub fn set_remote_url(repo: &Path, url: &str) -> Result<()> {
    if remote_url(repo)?.is_some() {
        git(repo, &["remote", "set-url", "origin", url])?;
    } else {
        git(repo, &["remote", "add", "origin", url])?;
    }
    Ok(())
}

/// True if the working tree has uncommitted changes or untracked files
/// (`git status --porcelain` is non-empty).
pub fn is_dirty(repo: &Path) -> Result<bool> {
    let out = git(repo, &["status", "--porcelain"])?;
    Ok(!out.is_empty())
}

/// True if any local branch has commits not on its upstream, or a local
/// branch has no upstream at all (its commits exist only here).
///
/// This is a safety check before deleting a repo, so when in doubt it says
/// `true`: a branch whose upstream is gone (or never set) counts as unpushed
/// if it has any commit that isn't on *some* remote-tracking branch.
pub fn has_unpushed(repo: &Path) -> Result<bool> {
    // One line per local branch: "<branch>\0<upstream>" (upstream may be
    // empty). NUL can't appear in ref names, so it's a safe separator.
    let refs = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(upstream)",
            "refs/heads",
        ],
    )?;

    // RUST: `.lines()` is an iterator over the lines of a string; a `for`
    // loop consumes it one item at a time.
    for line in refs.lines() {
        // RUST: `split_once` returns `Option<(&str, &str)>`; `unwrap_or`
        // supplies a fallback when the separator isn't found.
        let (branch, upstream) = line.split_once('\0').unwrap_or((line, ""));
        if branch.is_empty() {
            continue;
        }

        // Does the upstream ref actually exist locally? (It may be configured
        // but deleted on the remote and pruned.)
        let upstream_exists = !upstream.is_empty()
            && git_output(repo, &["rev-parse", "--verify", "--quiet", upstream])?
                .status
                .success();

        let count = if upstream_exists {
            // Commits reachable from the branch but not from its upstream.
            let range = format!("{upstream}..{branch}");
            git(repo, &["rev-list", "--count", &range])?
        } else {
            // No usable upstream: commits not on any remote-tracking branch.
            git(repo, &["rev-list", "--count", branch, "--not", "--remotes"])?
        };

        // RUST: `parse::<u64>()` converts text to a number and returns a
        // `Result`, since the text might not be a valid number.
        let count: u64 = count
            .parse()
            .with_context(|| format!("unexpected `git rev-list --count` output: {count:?}"))?;
        if count > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// True if `git stash list` is non-empty.
pub fn has_stash(repo: &Path) -> Result<bool> {
    let out = git(repo, &["stash", "list"])?;
    Ok(!out.is_empty())
}

/// `(ahead, behind)` counts of HEAD vs. its upstream
/// (`git rev-list --left-right --count HEAD...@{u}`), or `None` if HEAD is
/// detached or has no upstream. Does not fetch.
pub fn ahead_behind(repo: &Path) -> Result<Option<(u32, u32)>> {
    // First make sure this is a git repo at all, so "not a repo" is reported
    // as an error rather than silently becoming `None` below.
    git(repo, &["rev-parse", "--git-dir"])?;

    // Fails when HEAD is detached, unborn, or its branch has no upstream.
    let has_upstream = git_output(repo, &["rev-parse", "--verify", "--quiet", "@{u}"])?
        .status
        .success();
    if !has_upstream {
        return Ok(None);
    }

    // With `A...B`, `--left-right --count` prints "<only in A>\t<only in B>".
    // We put the upstream on the left, so the output is "behind\tahead".
    let out = git(
        repo,
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
    )?;
    let mut parts = out.split_whitespace();
    // RUST: `.next()` pulls the next item out of an iterator, returning
    // `None` when it's exhausted. `.and_then` chains another fallible step,
    // and `.ok()` turns a `Result` into an `Option` (dropping the error).
    let behind = parts.next().and_then(|s| s.parse::<u32>().ok());
    let ahead = parts.next().and_then(|s| s.parse::<u32>().ok());
    match (ahead, behind) {
        (Some(ahead), Some(behind)) => Ok(Some((ahead, behind))),
        _ => bail!("unexpected `git rev-list --left-right --count` output: {out:?}"),
    }
}

/// Find git repositories under `root`, descending at most `max_depth`
/// directory levels (`root` itself is depth 0). A directory containing a
/// `.git` entry (dir or file) is a repo; do not descend into repos. Skips
/// symlinks. Returns sorted absolute paths; a missing `root` yields `[]`.
pub fn find_repos(root: &Path, max_depth: usize) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        // RUST: `Vec::new()` creates an empty, growable list.
        return Ok(Vec::new());
    }
    let root = std::path::absolute(root)
        .with_context(|| format!("failed to make {} absolute", root.display()))?;

    let mut repos = Vec::new();
    // RUST: `&mut repos` lends the vector to the function *mutably*, so it
    // can push into it while we still own it here.
    walk(&root, 0, max_depth, &mut repos)?;
    repos.sort();
    Ok(repos)
}

/// Recursive helper for `find_repos`: record `dir` if it's a repo, otherwise
/// look inside it (unless we've hit `max_depth`).
fn walk(dir: &Path, depth: usize, max_depth: usize, repos: &mut Vec<PathBuf>) -> Result<()> {
    // `symlink_metadata` doesn't follow symlinks, so `.git` may be a dir or a
    // file (worktrees/submodules use a `.git` file) - either counts.
    if fs::symlink_metadata(dir.join(".git")).is_ok() {
        repos.push(dir.to_path_buf());
        return Ok(());
    }
    if depth >= max_depth {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // Directories we can't read (e.g. owned by root) are skipped rather
        // than aborting the whole scan.
        Err(e) if e.kind() == ErrorKind::PermissionDenied => return Ok(()),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read directory {}", dir.display()));
        }
    };

    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        // `DirEntry::file_type` does not follow symlinks, so a symlink to a
        // directory reports as a symlink and is skipped here.
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        // Skip hidden directories (`.cache`, `.git`, ...).
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        walk(&entry.path(), depth + 1, max_depth, repos)?;
    }
    Ok(())
}

/// Reasons it would lose work to delete the checkout at `repo`: uncommitted
/// changes, stashes, or unpushed commits. An empty list means it's safe.
///
/// Shared by `prune` and `remove --delete` so both apply the same safety rules.
// RUST: `&'static str` is a string literal baked into the binary, so it can be
// returned without anyone having to own it.
pub fn unsaved_work(repo: &Path) -> Result<Vec<&'static str>> {
    let mut reasons = Vec::new();
    if is_dirty(repo)? {
        reasons.push("uncommitted changes");
    }
    if has_stash(repo)? {
        reasons.push("stashes");
    }
    if has_unpushed(repo)? {
        reasons.push("unpushed commits");
    }
    Ok(reasons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Run a git command in `dir` for test setup, panicking on failure.
    /// Pins the default branch and never reads the developer's global config.
    fn run(dir: &Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Local identity + no signing, so commits work without global config.
    fn configure(repo: &Path) {
        run(repo, &["config", "--local", "user.name", "Test"]);
        run(
            repo,
            &["config", "--local", "user.email", "test@example.com"],
        );
        run(repo, &["config", "--local", "commit.gpgsign", "false"]);
    }

    /// `git init` a new working repo at `path`.
    fn init_repo(path: &Path) {
        fs::create_dir_all(path).expect("mkdir");
        run(path, &["init", "-q"]);
        configure(path);
    }

    fn commit(repo: &Path, msg: &str) {
        run(repo, &["commit", "-q", "--allow-empty", "-m", msg]);
    }

    /// A bare "remote" with one commit on `main`, plus a clone of it with
    /// `main` tracking `origin/main`. Returns (tempdir, bare path, clone path).
    fn remote_and_clone() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let bare = tmp.path().join("remote.git");
        let seed = tmp.path().join("seed");
        let work = tmp.path().join("work");

        fs::create_dir_all(&bare).expect("mkdir");
        run(&bare, &["init", "-q", "--bare"]);
        init_repo(&seed);
        commit(&seed, "initial");
        run(&seed, &["push", "-q", bare.to_str().expect("utf8"), "main"]);

        clone(bare.to_str().expect("utf8"), &work, None).expect("clone");
        configure(&work);
        (tmp, bare, work)
    }

    #[test]
    fn clone_creates_parent_dirs_and_checks_out_branch() {
        let (tmp, bare, work) = remote_and_clone();
        run(&work, &["checkout", "-q", "-b", "feature"]);
        commit(&work, "feature work");
        run(&work, &["push", "-q", "origin", "feature"]);

        let dest = tmp.path().join("deep/nested/dir/repo");
        clone(bare.to_str().expect("utf8"), &dest, Some("feature")).expect("clone");
        assert!(dest.join(".git").is_dir());
        assert_eq!(run(&dest, &["branch", "--show-current"]), "feature");
    }

    #[test]
    fn clone_failure_is_an_error() {
        let tmp = TempDir::new().expect("tempdir");
        let missing = tmp.path().join("nope.git");
        let err = clone(missing.to_str().expect("utf8"), &tmp.path().join("x"), None);
        assert!(err.is_err());
    }

    #[test]
    fn config_get_set_unset_roundtrip() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        let repo = tmp.path();

        assert_eq!(config_get(repo, "grm.test").expect("get"), None);
        config_set(repo, "grm.test", "hello world").expect("set");
        assert_eq!(
            config_get(repo, "grm.test").expect("get"),
            Some("hello world".to_string())
        );
        config_unset(repo, "grm.test").expect("unset");
        assert_eq!(config_get(repo, "grm.test").expect("get"), None);
        // Unsetting again is fine.
        config_unset(repo, "grm.test").expect("unset again");
    }

    #[test]
    fn config_get_outside_repo_is_error() {
        let tmp = TempDir::new().expect("tempdir");
        assert!(config_get(tmp.path(), "user.name").is_err());
    }

    #[test]
    fn remote_url_add_and_set() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        let repo = tmp.path();

        assert_eq!(remote_url(repo).expect("get"), None);
        set_remote_url(repo, "https://example.com/a.git").expect("add");
        assert_eq!(
            remote_url(repo).expect("get").as_deref(),
            Some("https://example.com/a.git")
        );
        set_remote_url(repo, "git@example.com:b.git").expect("set");
        assert_eq!(
            remote_url(repo).expect("get").as_deref(),
            Some("git@example.com:b.git")
        );
    }

    #[test]
    fn is_dirty_detects_untracked_and_modified() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        let repo = tmp.path();
        commit(repo, "initial");
        assert!(!is_dirty(repo).expect("dirty"));

        fs::write(repo.join("file.txt"), "a").expect("write");
        assert!(is_dirty(repo).expect("dirty"));

        run(repo, &["add", "file.txt"]);
        commit(repo, "add file");
        assert!(!is_dirty(repo).expect("dirty"));

        fs::write(repo.join("file.txt"), "b").expect("write");
        assert!(is_dirty(repo).expect("dirty"));
    }

    #[test]
    fn has_stash_detects_stash() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        let repo = tmp.path();
        fs::write(repo.join("f"), "a").expect("write");
        run(repo, &["add", "f"]);
        commit(repo, "initial");
        assert!(!has_stash(repo).expect("stash"));

        fs::write(repo.join("f"), "b").expect("write");
        run(repo, &["stash", "-q"]);
        assert!(has_stash(repo).expect("stash"));
    }

    #[test]
    fn has_unpushed_fresh_clone_is_clean() {
        let (_tmp, _bare, work) = remote_and_clone();
        assert!(!has_unpushed(&work).expect("unpushed"));
    }

    #[test]
    fn has_unpushed_commit_ahead_of_upstream() {
        let (_tmp, _bare, work) = remote_and_clone();
        commit(&work, "local only");
        assert!(has_unpushed(&work).expect("unpushed"));
        run(&work, &["push", "-q"]);
        assert!(!has_unpushed(&work).expect("unpushed"));
    }

    #[test]
    fn has_unpushed_branch_without_upstream() {
        let (_tmp, _bare, work) = remote_and_clone();
        // A new branch with no new commits is fully on origin/main: not unpushed.
        run(&work, &["checkout", "-q", "-b", "topic"]);
        assert!(!has_unpushed(&work).expect("unpushed"));
        // Once it has its own commit, it is.
        commit(&work, "topic work");
        assert!(has_unpushed(&work).expect("unpushed"));
    }

    #[test]
    fn has_unpushed_repo_without_remote() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        // Empty repo (no branches yet): nothing to lose.
        assert!(!has_unpushed(tmp.path()).expect("unpushed"));
        commit(tmp.path(), "only here");
        assert!(has_unpushed(tmp.path()).expect("unpushed"));
    }

    #[test]
    fn has_unpushed_upstream_gone() {
        let (_tmp, _bare, work) = remote_and_clone();
        run(&work, &["checkout", "-q", "-b", "topic"]);
        commit(&work, "topic work");
        run(&work, &["push", "-q", "-u", "origin", "topic"]);
        assert!(!has_unpushed(&work).expect("unpushed"));
        // Delete the branch on the remote and prune the tracking ref.
        run(&work, &["push", "-q", "origin", "--delete", "topic"]);
        assert!(has_unpushed(&work).expect("unpushed"));
    }

    #[test]
    fn ahead_behind_counts() {
        let (tmp, bare, work) = remote_and_clone();
        assert_eq!(ahead_behind(&work).expect("ab"), Some((0, 0)));

        commit(&work, "local 1");
        commit(&work, "local 2");
        assert_eq!(ahead_behind(&work).expect("ab"), Some((2, 0)));

        // Someone else pushes one commit to the remote.
        let other = tmp.path().join("other");
        clone(bare.to_str().expect("utf8"), &other, None).expect("clone");
        configure(&other);
        commit(&other, "remote 1");
        run(&other, &["push", "-q"]);
        run(&work, &["fetch", "-q"]);
        assert_eq!(ahead_behind(&work).expect("ab"), Some((2, 1)));
    }

    #[test]
    fn ahead_behind_none_without_upstream_or_detached() {
        let (_tmp, _bare, work) = remote_and_clone();
        run(&work, &["checkout", "-q", "-b", "topic"]);
        assert_eq!(ahead_behind(&work).expect("ab"), None);
        run(&work, &["checkout", "-q", "--detach"]);
        assert_eq!(ahead_behind(&work).expect("ab"), None);

        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        assert_eq!(ahead_behind(tmp.path()).expect("ab"), None);
    }

    #[test]
    fn ahead_behind_not_a_repo_is_error() {
        let tmp = TempDir::new().expect("tempdir");
        assert!(ahead_behind(tmp.path()).is_err());
    }

    #[test]
    fn find_repos_walks_to_depth_and_skips_hidden() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        init_repo(&root.join("a"));
        init_repo(&root.join("org/b"));
        init_repo(&root.join("org/c"));
        init_repo(&root.join("x/y/z/too-deep"));
        init_repo(&root.join(".hidden/d"));
        // A nested repo inside a repo is not reported (we don't descend).
        init_repo(&root.join("a/vendor/nested"));
        // A `.git` *file* (like a worktree) also marks a repo.
        fs::create_dir_all(root.join("org/wt")).expect("mkdir");
        fs::write(root.join("org/wt/.git"), "gitdir: /elsewhere").expect("write");
        // Plain files are ignored.
        fs::write(root.join("README"), "hi").expect("write");

        let root_abs = std::path::absolute(root).expect("abs");
        let found = find_repos(root, 3).expect("find");
        assert_eq!(
            found,
            vec![
                root_abs.join("a"),
                root_abs.join("org/b"),
                root_abs.join("org/c"),
                root_abs.join("org/wt"),
            ]
        );

        let shallow = find_repos(root, 1).expect("find");
        assert_eq!(shallow, vec![root_abs.join("a")]);
    }

    #[test]
    fn find_repos_root_is_repo_and_missing_root() {
        let tmp = TempDir::new().expect("tempdir");
        init_repo(tmp.path());
        let abs = std::path::absolute(tmp.path()).expect("abs");
        assert_eq!(find_repos(tmp.path(), 0).expect("find"), vec![abs]);

        let missing = tmp.path().join("does-not-exist");
        assert!(find_repos(&missing, 3).expect("find").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn find_repos_skips_symlinks() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        init_repo(&outside.join("repo"));
        fs::create_dir_all(&root).expect("mkdir");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
        assert!(find_repos(&root, 3).expect("find").is_empty());
    }
}
