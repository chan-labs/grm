# grm

**Manage your git repos declaratively.** `grm` works like `package.json` for
your git checkouts. You list the repos you want in a `grm.yml` manifest, and
`grm sync` clones the missing ones, puts each in a predictable place, and sets
the right git identity (name, email, signing key) in each one. `grm status`
shows where your disk differs from the manifest.

```yaml
# ~/.config/grm/grm.yml
root: ~/workspace
defaults:
  identity:
    name: Jane Doe
    email: jane@example.com
repos:
  - rust-lang/cargo                 # -> ~/workspace/cargo
  - url: git@gitlab.com:work/api.git
    identity:
      email: jane@work.example      # work email for this repo only
```

```sh
grm sync     # clone what's missing and fix git config
grm status   # show differences (exit code 1 if sync has work to do)
```

## Contents

- [Install](#install)
- [Quickstart](#quickstart)
- [Manifest reference](#manifest-reference)
- [Command reference](#command-reference)
- [Safety](#safety)
- [Known limitations](#known-limitations)
- [Learning and contributing](#learning-and-contributing)

## Install

**Install script** (downloads a prebuilt binary):

```sh
curl -fsSL https://raw.githubusercontent.com/chan-labs/grm/main/install.sh | sh
```

**[mise](https://mise.jdx.dev)** (from GitHub releases):

```sh
mise use -g github:chan-labs/grm
```

**Build from source.** The Rust toolchain is pinned in `mise.toml`:

```sh
git clone https://github.com/chan-labs/grm
cd grm
mise install                       # installs the pinned Rust toolchain
mise exec -- cargo install --path .
```

`grm` runs the `git` command-line tool, so `git` must be on your `PATH`.

## Quickstart

### New machine

1. Install `grm` (see above).
2. Put your manifest at `~/.config/grm/grm.yml`, for example by copying it
   from your dotfiles.
3. Preview and apply:

   ```sh
   grm status   # what's missing or different?
   grm sync     # clone it all and set up identities
   ```

### Existing machine (repos already checked out)

Create a manifest, then fill it from the repos you already have:

```sh
grm init                 # writes ~/.config/grm/grm.yml with root ~/workspace
grm import --dry-run     # preview what would be added
grm import
grm status
```

`grm init` writes the global manifest `~/.config/grm/grm.yml` (the same path
on Linux and macOS), so grm finds it from any directory. It sets `root` to
`~/workspace` (change it with `--root ~/code`) and fills in
`defaults.identity` from the `user.name` / `user.email` in your global git
config, if they are set. To create the manifest somewhere else, pass
`-f path/to/grm.yml` (or set `GRM_FILE`); see
[Manifest discovery](#manifest-discovery) for when grm finds it again.

From then on, use `grm add owner/name` instead of `git clone` so the manifest
stays up to date.

## Manifest reference

A complete, commented example is in [`grm.example.yml`](grm.example.yml).

### `root` (required)

The directory repos are cloned under. It must be an absolute path or start
with `~`. Relative roots are rejected.

```yaml
root: ~/workspace
```

### `defaults` (optional)

These apply to every repo unless the repo overrides them.

| Field | Default | Meaning |
| --- | --- | --- |
| `host` | `github.com` | Host used for `owner/name` shorthand. |
| `protocol` | `ssh` | `ssh` or `https`. Sets how clone URLs are built from shorthand. |
| `identity` | none | The base git identity. See [Identity](#identity). |

Shorthand specs become URLs like this:

| `protocol` | URL |
| --- | --- |
| `ssh` | `git@{host}:{owner}/{name}.git` |
| `https` | `https://{host}/{owner}/{name}.git` |

URLs you write out in full are used as given.

### `repos`

Each entry is either a **short** string or a **full** mapping.

**Short form:** a repo spec, which can be any of the following.

| Spec | Example |
| --- | --- |
| `owner/name` | `rust-lang/cargo` (host and protocol come from `defaults`) |
| `host/owner/name` | `gitlab.com/me/tool` |
| SSH (scp-style) | `git@github.com:rust-lang/cargo.git` |
| SSH URL | `ssh://git@github.com:22/rust-lang/cargo.git` |
| HTTPS URL | `https://github.com/rust-lang/cargo` |
| Local repo | `file:///srv/git/me/tool.git` or `/srv/git/me/tool.git` (mostly for testing) |

A bare `name` with no owner is an error. A trailing `.git` or `/` is ignored
when working out the name. For nested groups, such as GitLab's
`group/sub/name`, everything between the host and the last segment counts as
the owner.

**Full form:** a mapping with these fields.

| Field | Required | Meaning |
| --- | --- | --- |
| `url` | yes | Any spec from the table above, not only full URLs. |
| `path` | no | Where to put the checkout. A relative path is resolved against `root`. Absolute paths and `~` paths are used as given. Default: `{root}/{name}`. |
| `branch` | no | The branch to check out when the repo is first cloned. |
| `identity` | no | Overrides merged over `defaults.identity`. |

```yaml
repos:
  - rust-lang/cargo         # -> ~/workspace/cargo
  - url: git@gitlab.com:work/api.git
    path: work/api          # -> ~/workspace/work/api
    branch: main
    identity:
      email: jane@work.example
```

**Checkout paths.** By default a repo goes to `{root}/{name}`: just the repo
name, with no host or owner directories. So two repos with the same name
(say `me/tool` and `gitlab.com/work/tool`) would both want `{root}/tool`;
give one of them a `path:`. `grm add` catches this and tells you to pass
`--path`.

Two entries that resolve to the same path, or to the same repo, are an error.
URLs are compared without their protocol, user, port, `.git` suffix or
letter case, so `git@github.com:Foo/Bar.git` and `https://github.com/foo/bar`
count as the same repo.

### Identity

```yaml
identity:
  name: Jane Doe
  email: jane@example.com
  signing:
    key: ~/.ssh/id_ed25519.pub   # SSH: path to the public key (~ expanded)
    format: ssh                  # ssh (default) or openpgp
```

For OpenPGP, `key` is the key ID. Quote it if it looks like a number, for
example `key: "0xDEADBEEF"`.

**Merging.** A repo's `identity` is merged over `defaults.identity` one field
at a time. Each field the repo sets wins, and any field it leaves out comes
from the defaults. `signing` is the exception: it is replaced as a whole, so
the key and format are never mixed from different places.

**Git config keys.** grm writes the merged identity to each repo's **local**
config (`.git/config`). It never changes your global `~/.gitconfig`.

| Key | Value |
| --- | --- |
| `user.name` | `name` |
| `user.email` | `email` |
| `user.signingkey` | `signing.key` (`~` expanded for SSH) |
| `gpg.format` | `signing.format` (`ssh` or `openpgp`) |
| `commit.gpgsign` | `true` if `signing` is set |
| `tag.gpgsign` | `true` if `signing` is set |

grm manages all six keys. If the merged identity doesn't set a value, grm
**unsets** that key in the repo's local config, so git falls back to your
global config. Values you set locally by hand for these keys show up as drift,
and `grm sync` removes them.

### Manifest discovery

grm uses the first manifest it finds, in this order:

1. `-f/--file <path>` or the `GRM_FILE` environment variable.
2. `grm.yml` in the current directory or any directory above it.
3. `~/.config/grm/grm.yml` (on every platform, macOS included).

If none of these exist, grm stops with an error that lists where it looked
and suggests `grm init`.

## Command reference

Global option: `-f, --file <PATH>` (or `GRM_FILE`) selects the manifest. It
can go before or after the subcommand. Every command exits with code 1 on an
error (and code 2 for invalid command-line usage, such as an unknown flag).

### `grm init`

Create a new manifest. It is written to `-f`/`GRM_FILE` if given, otherwise to
`~/.config/grm/grm.yml`. It contains `root: ~/workspace` (or `--root`),
`defaults.identity` filled in from your global git config's `user.name` and
`user.email` (if set), and no repos.

| Flag | Meaning |
| --- | --- |
| `--root <DIR>` | The root to store in the manifest. Default `~/workspace`. Must be absolute or start with `~`. |
| `--force` | Overwrite an existing manifest. |

```sh
grm init
grm init --root ~/code
grm -f ./grm.yml init     # a project-local manifest instead
```

### `grm add <SPEC>`

Add a repo to the manifest, then clone it and apply its identity. `SPEC` can
be any form from the [repos](#repos) table.

These are errors, and leave the manifest unchanged:

- the repo is already listed;
- another entry already uses the checkout path (pass `--path` to put it
  somewhere else);
- the target path exists and is not an empty directory or a checkout of the
  same repo (for example an unrelated directory, or a checkout of a
  different repo). This check is skipped with `--no-clone`.

If the target path is already a checkout of this repo, grm doesn't clone and
just applies the identity. If cloning fails, the entry stays in the manifest
and `grm sync` retries it.

| Flag | Meaning |
| --- | --- |
| `--path <PATH>` | Custom checkout path (relative to root, absolute, or `~`). |
| `--name <NAME>` | `user.name` for this repo. |
| `--email <EMAIL>` | `user.email` for this repo. |
| `--branch <BRANCH>` | The branch to check out on clone. |
| `--no-clone` | Only update the manifest. |

The entry is saved in the short form unless you pass one of `--path`,
`--name`, `--email` or `--branch`.

```sh
grm add rust-lang/cargo
grm add git@gitlab.com:work/api.git --path work/api --email jane@work.example
grm add me/notes --no-clone
```

### `grm remove <SPEC>`

Remove a repo from the manifest. `SPEC` is matched as a repo spec first (so
any URL form of the same repo works). If that finds nothing, it is matched
against the repo name (for example `cargo`) or the entry text as written; a
name that matches several repos is an error.

By default the checkout stays on disk, where it becomes an "extra" that
`grm prune` would delete.

| Flag | Meaning |
| --- | --- |
| `--delete` | Also delete the checkout. |
| `--force` | With `--delete`: delete even if the repo has uncommitted changes, unpushed commits or stashes (or isn't a git checkout). |
| `-y, --yes` | Don't ask for confirmation. |

The safety checks and the confirmation happen before the manifest is
changed, so a refusal or a "no" leaves everything as it was.

```sh
grm remove rust-lang/cargo
grm remove cargo --delete
```

### `grm sync`

Make the disk match the manifest:

- Missing repos are cloned in parallel, then their identity is applied.
- Wrong or extra identity config keys are set or unset.
- A wrong or missing `origin` remote is changed to the manifest URL.
- Uncommitted changes and ahead/behind commits are only reported. grm never
  commits, pulls, pushes or resets.

A repo that fails is reported and the others still run.

| Flag | Meaning |
| --- | --- |
| `--dry-run` | Print the planned actions without changing anything. |
| `-j, --jobs <N>` | The number of parallel clones. Default 4. |
| `--prune` | Afterwards, also delete extras, as `grm prune` does. There is no `--force` here: extras with unsaved work are always skipped. |
| `-y, --yes` | Don't ask for confirmation (when pruning). |

Exits with code 1 if any repo failed, or if a `--prune` deletion failed.

```sh
grm sync --dry-run
grm sync -j 8
grm sync --prune
```

### `grm status` (alias `grm diff`)

Show how the disk differs from the manifest. This doesn't change anything and
doesn't fetch. For each repo it reports one of:

- **missing** (`✗`): not cloned yet (there's no git checkout at its path).
- **ok** (`✓`): matches the manifest.
- **drifted** (`~`): wrong identity config or a wrong `origin`, which
  `grm sync` would fix.

Uncommitted changes and ahead/behind counts are listed under the repo as
well, dimmed, but they are informational only: a repo whose only differences
are these still counts as ok.

It also lists **extras**: git repos under `root` (up to 3 levels deep) that
aren't in the manifest.

**Exit code:** 1 if any repo is missing or drifted (config or `origin`), or if
there are extras; otherwise 0. Uncommitted changes and ahead/behind on their
own don't change the exit code. This makes it useful in scripts and CI:

```sh
grm status || echo "repos out of sync"
grm diff
```

### `grm prune`

Delete the extras: repos under `root` that aren't in the manifest. grm lists
them and asks once for confirmation first. Repos with uncommitted changes,
unpushed commits or stashes (or where those checks fail) are skipped with a
warning unless you pass `--force`. Skipped repos and answering "no" are not
errors; the exit code is 1 only if a deletion failed.

| Flag | Meaning |
| --- | --- |
| `--dry-run` | List what would be deleted. |
| `-y, --yes` | Don't ask for confirmation. |
| `--force` | Also delete repos with unsaved work. |

```sh
grm prune --dry-run
grm prune
```

### `grm import [DIR]`

Scan a directory (default: the manifest `root`) for existing git checkouts and
add them to the manifest. Nothing is cloned or deleted.

- Repos without an `origin` remote, with an `origin` grm can't parse, or
  already in the manifest are skipped.
- `url` is written as `owner/name` when that shorthand expands (with your
  `defaults`) to the same URL as `origin`; otherwise the `origin` URL is used.
- Repos at the default `{root}/{name}` path are added without a `path`.
  Others get a `path`, relative to `root` when they are under it.
- A local `user.name` / `user.email` that differs from `defaults.identity` is
  kept as that repo's `identity`.

Entries with no `path` or `identity` are written in the short form.

| Flag | Meaning |
| --- | --- |
| `--depth <N>` | How many directory levels to search. Default 2, which covers the flat `{root}/{name}` layout plus one level of grouping folders. |
| `--dry-run` | Print what would be added without saving. |

```sh
grm import --dry-run
grm import ~/old-projects --depth 3
```

## Safety

grm is built so that it can't lose your work by accident:

- **Unsaved work is never deleted without `--force`.** `grm prune` and
  `grm remove --delete` refuse to delete a repo with uncommitted changes or
  untracked files, stashes, or unpushed commits. A local branch with no
  upstream counts as unpushed. If grm can't run these checks, it doesn't
  delete. `grm sync --prune` has no `--force` at all, so it always skips such
  repos. The checks live in one place, `git::unsaved_work`, shared by every
  command that deletes.
- **Deleting always asks first.** Every deletion asks for confirmation
  (`[y/N]`, where the default is no; an empty answer or closed stdin counts as
  no) unless you pass `-y/--yes`.
- **`--dry-run`** on `sync`, `prune` and `import` shows what would happen
  without changing anything.
- **`grm status` is read-only.** `sync` changes only what the manifest
  manages: it clones, sets the `origin` remote and sets local identity
  config. It never touches your branches, working tree or global git config.
- The manifest is saved atomically (written to a temporary file, then
  renamed), so an interrupted save can't leave half a file behind.

## Known limitations

- **Comments in `grm.yml` are lost** when `grm add`, `grm remove` or
  `grm import` rewrite the file. If you rely on comments, edit the manifest
  by hand and use only `sync`/`status`/`prune`, which never write to it.
- `status` and `sync` don't fetch, so ahead/behind counts are only as current
  as your last `git fetch`.
- `branch` only takes effect on the first clone. grm doesn't switch branches
  in existing checkouts.
- A path that exists but isn't a git checkout is reported as **missing**, and
  `grm sync` then fails to clone into it (unless the directory is empty)
  with git's "already exists" error.
  Move the directory away or give the repo a different `path:`.
- Parallel clones (`sync -j`) print git's progress output interleaved.

## Learning and contributing

grm is also a project for learning Rust, and the code is commented with that
in mind. [LEARNING.md](LEARNING.md) is a guided tour: setup, the order to read
the files in, an index of Rust concepts with links to where each one appears,
and exercises. See [CONTRIBUTING.md](CONTRIBUTING.md) for the code layout,
conventions and checks to run before committing.
