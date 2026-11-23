//! End-to-end tests: run the real `grm` binary against local bare repos.
//!
//! Every test uses a fresh [`common::Fixture`] (temp dir, fake HOME, empty
//! global git config), so nothing touches the real home directory or the
//! network. Assertions focus on exit codes, the filesystem and
//! `git config --local` values; stdout checks are deliberately loose
//! (usually just "mentions the repo name") since the output format isn't
//! fixed.

// RUST: `mod common;` pulls in `tests/common/mod.rs` as a module.
mod common;

use std::fs;

use common::Fixture;
use predicates::prelude::*;

/// Fixture with bare remotes `alpha` and `beta`, both listed in the manifest
/// with a default identity.
fn two_repo_fixture() -> Fixture {
    let fx = Fixture::new();
    let a = fx.bare_repo("alpha");
    let b = fx.bare_repo("beta");
    fx.write_simple_manifest(&[&a, &b]);
    fx
}

/// Combined stdout + stderr of a finished command, for loose "mentions X"
/// assertions that don't care which stream the text went to.
fn all_output(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ---------------------------------------------------------------- sync

#[test]
fn sync_clones_all_repos_and_sets_identity() {
    let fx = Fixture::new();
    let a = fx.bare_repo("alpha");
    let b = fx.bare_repo("beta");
    let key = fx.base.join("id_ed25519.pub");
    fs::write(&key, "ssh-ed25519 AAAATEST test@example.com\n").unwrap();
    fx.write_manifest(&format!(
        "defaults:\n  identity:\n    name: Jane Doe\n    email: jane@example.com\n    signing:\n      key: {}\n      format: ssh\nrepos:\n  - {a}\n  - {b}\n",
        key.display()
    ));

    fx.grm().arg("sync").assert().success();

    for name in ["alpha", "beta"] {
        let repo = fx.repo_path(name);
        assert!(repo.join(".git").exists(), "{name} was not cloned");
        assert!(repo.join("README.md").exists(), "{name} has no checkout");
        assert_eq!(fx.config(&repo, "user.name").as_deref(), Some("Jane Doe"));
        assert_eq!(
            fx.config(&repo, "user.email").as_deref(),
            Some("jane@example.com")
        );
        assert_eq!(
            fx.config(&repo, "user.signingkey").as_deref(),
            Some(key.to_str().unwrap())
        );
        assert_eq!(fx.config(&repo, "gpg.format").as_deref(), Some("ssh"));
        assert_eq!(fx.config(&repo, "commit.gpgsign").as_deref(), Some("true"));
        assert_eq!(fx.config(&repo, "tag.gpgsign").as_deref(), Some("true"));
    }
    assert_eq!(fx.origin(&fx.repo_path("alpha")), Some(a));
}

#[test]
fn per_repo_identity_overrides_defaults() {
    let fx = Fixture::new();
    let a = fx.bare_repo("alpha");
    let b = fx.bare_repo("beta");
    fx.write_manifest(&format!(
        "defaults:\n  identity:\n    name: Jane Doe\n    email: jane@example.com\nrepos:\n  - {a}\n  - url: {b}\n    identity:\n      email: jane@work.example\n"
    ));

    fx.grm().arg("sync").assert().success();

    let alpha = fx.repo_path("alpha");
    let beta = fx.repo_path("beta");
    assert_eq!(
        fx.config(&alpha, "user.email").as_deref(),
        Some("jane@example.com")
    );
    assert_eq!(
        fx.config(&beta, "user.email").as_deref(),
        Some("jane@work.example")
    );
    // Fields the override leaves out come from the defaults.
    assert_eq!(fx.config(&beta, "user.name").as_deref(), Some("Jane Doe"));
    // No signing configured anywhere: grm leaves these unset.
    assert_eq!(fx.config(&beta, "user.signingkey"), None);
    assert_eq!(fx.config(&beta, "commit.gpgsign"), None);
}

#[test]
fn sync_fixes_config_drift_and_wrong_origin() {
    let fx = two_repo_fixture();
    let other = fx.bare_repo("other");
    fx.grm().arg("sync").assert().success();

    let alpha = fx.repo_path("alpha");
    let beta = fx.repo_path("beta");
    fx.git(&alpha, &["config", "user.email", "wrong@example.com"]);
    fx.git(&alpha, &["config", "commit.gpgsign", "true"]); // not managed by manifest -> unset
    fx.git(&beta, &["remote", "set-url", "origin", &other]);

    fx.grm().arg("sync").assert().success();

    assert_eq!(
        fx.config(&alpha, "user.email").as_deref(),
        Some("default@example.com")
    );
    assert_eq!(fx.config(&alpha, "commit.gpgsign"), None);
    assert_eq!(fx.origin(&beta), Some(fx.url("beta")));

    // And now everything matches.
    fx.grm().arg("status").assert().code(0);
}

#[test]
fn sync_dry_run_changes_nothing() {
    let fx = two_repo_fixture();
    // alpha already exists (with drifted config); beta is missing.
    let alpha = fx.repo_path("alpha");
    fx.clone_to(&fx.url("alpha"), &alpha);
    fx.git(&alpha, &["config", "user.email", "wrong@example.com"]);
    let manifest_before = fx.read_manifest();

    fx.grm().args(["sync", "--dry-run"]).assert().success();

    assert!(
        !fx.repo_path("beta").exists(),
        "dry-run must not clone missing repos"
    );
    assert_eq!(
        fx.config(&alpha, "user.email").as_deref(),
        Some("wrong@example.com"),
        "dry-run must not change git config"
    );
    assert_eq!(fx.config(&alpha, "user.name"), None);
    assert_eq!(fx.read_manifest(), manifest_before);
}

// ---------------------------------------------------------------- status

#[test]
fn status_exits_0_when_everything_matches() {
    let fx = two_repo_fixture();
    fx.grm().arg("sync").assert().success();
    fx.grm().arg("status").assert().code(0);
    // `diff` is an alias.
    fx.grm().arg("diff").assert().code(0);
}

#[test]
fn status_exits_1_when_email_changed_by_hand() {
    let fx = two_repo_fixture();
    fx.grm().arg("sync").assert().success();
    fx.git(
        &fx.repo_path("beta"),
        &["config", "user.email", "hand@example.com"],
    );

    fx.grm()
        .arg("status")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("beta"));
    // status is read-only.
    assert_eq!(
        fx.config(&fx.repo_path("beta"), "user.email").as_deref(),
        Some("hand@example.com")
    );
}

#[test]
fn status_exits_1_when_repo_missing() {
    let fx = two_repo_fixture();
    fx.grm().arg("sync").assert().success();
    fs::remove_dir_all(fx.repo_path("alpha")).unwrap();

    fx.grm()
        .arg("status")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("alpha"));
    assert!(!fx.repo_path("alpha").exists(), "status must not clone");
}

#[test]
fn status_exits_1_when_extra_repo_under_root() {
    let fx = two_repo_fixture();
    let stray = fx.bare_repo("stray");
    fx.grm().arg("sync").assert().success();
    fx.clone_to(&stray, &fx.root.join("stray"));

    fx.grm()
        .arg("status")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("stray"));
    assert!(fx.root.join("stray").exists(), "status must not delete");
}

// ---------------------------------------------------------------- prune

#[test]
fn prune_yes_removes_clean_extra() {
    let fx = two_repo_fixture();
    let stray = fx.bare_repo("stray");
    fx.grm().arg("sync").assert().success();
    let extra = fx.root.join("stray");
    fx.clone_to(&stray, &extra);

    fx.grm().args(["prune", "--yes"]).assert().success();

    assert!(!extra.exists(), "clean extra should be deleted");
    assert!(fx.repo_path("alpha").exists(), "manifest repos must stay");
    assert!(fx.repo_path("beta").exists(), "manifest repos must stay");
}

#[test]
fn prune_keeps_dirty_extra_without_force() {
    let fx = two_repo_fixture();
    let stray = fx.bare_repo("stray");
    let extra = fx.root.join("stray");
    fx.clone_to(&stray, &extra);
    fs::write(extra.join("untracked.txt"), "work in progress\n").unwrap();

    let out = fx.grm().args(["prune", "--yes"]).output().unwrap();
    assert!(
        extra.exists(),
        "dirty extra must not be deleted without --force"
    );
    assert!(
        all_output(&out).contains("stray"),
        "prune should warn about the skipped repo: {}",
        all_output(&out)
    );
}

#[test]
fn prune_keeps_extra_with_unpushed_commits_without_force() {
    let fx = two_repo_fixture();
    let stray = fx.bare_repo("stray");
    let extra = fx.root.join("stray");
    fx.clone_to(&stray, &extra);
    fs::write(extra.join("new.txt"), "local only\n").unwrap();
    fx.commit_all(&extra, "unpushed work");

    let out = fx.grm().args(["prune", "--yes"]).output().unwrap();
    assert!(
        extra.exists(),
        "extra with unpushed commits must not be deleted without --force"
    );
    assert!(
        all_output(&out).contains("stray"),
        "prune should warn about the skipped repo: {}",
        all_output(&out)
    );
}

#[test]
fn prune_force_removes_dirty_and_unpushed_extras() {
    let fx = two_repo_fixture();
    let dirty_url = fx.bare_repo("dirty");
    let ahead_url = fx.bare_repo("ahead");
    let dirty = fx.root.join("dirty");
    let ahead = fx.root.join("ahead");
    fx.clone_to(&dirty_url, &dirty);
    fs::write(dirty.join("untracked.txt"), "wip\n").unwrap();
    fx.clone_to(&ahead_url, &ahead);
    fs::write(ahead.join("new.txt"), "local only\n").unwrap();
    fx.commit_all(&ahead, "unpushed work");

    fx.grm()
        .args(["prune", "--yes", "--force"])
        .assert()
        .success();

    assert!(!dirty.exists(), "--force should delete the dirty extra");
    assert!(!ahead.exists(), "--force should delete the unpushed extra");
}

#[test]
fn prune_dry_run_keeps_everything() {
    let fx = two_repo_fixture();
    let stray = fx.bare_repo("stray");
    let extra = fx.root.join("stray");
    fx.clone_to(&stray, &extra);

    fx.grm()
        .args(["prune", "--dry-run", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stray"));
    assert!(extra.exists(), "--dry-run must not delete");
}

// ---------------------------------------------------------------- add

#[test]
fn add_no_clone_appends_to_manifest() {
    let fx = two_repo_fixture();
    let gamma = fx.bare_repo("gamma");

    fx.grm()
        .args(["add", &gamma, "--no-clone"])
        .assert()
        .success();

    let manifest = fx.read_manifest();
    assert!(manifest.contains(&gamma), "manifest:\n{manifest}");
    // Existing entries are kept.
    assert!(manifest.contains(&fx.url("alpha")), "manifest:\n{manifest}");
    assert!(manifest.contains(&fx.url("beta")), "manifest:\n{manifest}");
    assert!(!fx.repo_path("gamma").exists(), "--no-clone must not clone");
}

#[test]
fn add_then_sync_clones() {
    let fx = two_repo_fixture();
    let gamma = fx.bare_repo("gamma");

    fx.grm()
        .args(["add", &gamma, "--no-clone"])
        .assert()
        .success();
    fx.grm().arg("sync").assert().success();

    let repo = fx.repo_path("gamma");
    assert!(repo.join(".git").exists(), "gamma was not cloned by sync");
    assert_eq!(fx.origin(&repo), Some(gamma));
    assert_eq!(
        fx.config(&repo, "user.email").as_deref(),
        Some("default@example.com")
    );
}

#[test]
fn add_clones_by_default() {
    let fx = two_repo_fixture();
    let gamma = fx.bare_repo("gamma");

    fx.grm().args(["add", &gamma]).assert().success();

    let repo = fx.repo_path("gamma");
    assert!(repo.join(".git").exists(), "add should clone");
    assert_eq!(
        fx.config(&repo, "user.name").as_deref(),
        Some("Default Name")
    );
}

#[test]
fn add_duplicate_fails() {
    let fx = two_repo_fixture();
    let before = fx.read_manifest();

    fx.grm()
        .args(["add", &fx.url("alpha"), "--no-clone"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panicked").not());

    assert_eq!(
        fx.read_manifest(),
        before,
        "failed add must not edit manifest"
    );
}

// ---------------------------------------------------------------- remove

#[test]
fn remove_edits_manifest_and_keeps_checkout() {
    let fx = two_repo_fixture();
    fx.grm().arg("sync").assert().success();

    fx.grm()
        .args(["remove", &fx.url("alpha")])
        .assert()
        .success();

    let manifest = fx.read_manifest();
    assert!(
        !manifest.contains(&fx.url("alpha")),
        "manifest:\n{manifest}"
    );
    assert!(manifest.contains(&fx.url("beta")), "manifest:\n{manifest}");
    assert!(
        fx.repo_path("alpha").exists(),
        "remove without --delete must keep the checkout"
    );
}

#[test]
fn remove_delete_yes_removes_dir() {
    let fx = two_repo_fixture();
    fx.grm().arg("sync").assert().success();

    fx.grm()
        .args(["remove", &fx.url("alpha"), "--delete", "--yes"])
        .assert()
        .success();

    assert!(!fx.read_manifest().contains(&fx.url("alpha")));
    assert!(
        !fx.repo_path("alpha").exists(),
        "checkout should be deleted"
    );
    assert!(fx.repo_path("beta").exists());
}

#[test]
fn remove_unknown_repo_fails() {
    let fx = two_repo_fixture();
    let before = fx.read_manifest();
    fx.grm()
        .args(["remove", &fx.url("nope")])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panicked").not());
    assert_eq!(fx.read_manifest(), before);
}

// ---------------------------------------------------------------- import

#[test]
fn import_adds_repo_with_origin_and_skips_repo_without() {
    let fx = Fixture::new();
    let existing = fx.bare_repo("existing");
    fx.write_manifest("repos: []\n");

    // A checkout with an origin, cloned by hand.
    fx.clone_to(&existing, &fx.root.join("existing"));
    // A repo with no origin remote.
    let lonely = fx.root.join("lonely");
    fs::create_dir_all(&lonely).unwrap();
    fx.git(&lonely, &["-c", "init.defaultBranch=main", "init", "-q"]);

    fx.grm().arg("import").assert().success();

    let manifest = fx.read_manifest();
    assert!(manifest.contains(&existing), "manifest:\n{manifest}");
    assert!(!manifest.contains("lonely"), "manifest:\n{manifest}");

    // Importing again adds nothing new (already listed).
    fx.grm().arg("import").assert().success();
    assert_eq!(
        fx.read_manifest().matches(existing.as_str()).count(),
        1,
        "re-import must not duplicate entries"
    );
}

#[test]
fn import_dry_run_leaves_manifest_alone() {
    let fx = Fixture::new();
    let existing = fx.bare_repo("existing");
    fx.write_manifest("repos: []\n");
    fx.clone_to(&existing, &fx.root.join("existing"));
    let before = fx.read_manifest();

    fx.grm()
        .args(["import", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("existing"));
    assert_eq!(fx.read_manifest(), before);
}

// ---------------------------------------------------------------- init

#[test]
fn init_creates_file_and_refuses_overwrite_without_force() {
    let fx = Fixture::new();
    let manifest = fx.base.join("conf").join("grm.yml");
    let root = fx.root.to_str().unwrap();
    let init = |extra: &[&str]| {
        let mut cmd = fx.grm_with_file(&manifest);
        cmd.arg("init").args(extra);
        cmd
    };

    init(&["--root", root]).assert().success();
    assert!(manifest.exists(), "init should create the manifest");
    let contents = fs::read_to_string(&manifest).unwrap();
    assert!(contents.contains(root), "manifest:\n{contents}");

    // Second init without --force fails and leaves the file alone.
    fs::write(&manifest, "root: /keep/me\n").unwrap();
    init(&["--root", root])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panicked").not());
    assert_eq!(fs::read_to_string(&manifest).unwrap(), "root: /keep/me\n");

    // With --force it overwrites.
    init(&["--root", root, "--force"]).assert().success();
    let contents = fs::read_to_string(&manifest).unwrap();
    assert!(contents.contains(root), "manifest:\n{contents}");

    // The new manifest is usable: an empty manifest with an empty root is in sync.
    fx.grm_with_file(&manifest).arg("status").assert().code(0);
}
