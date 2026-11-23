# grm

**Manage your git repos declaratively.** `grm` works like `package.json` for
your git checkouts. You list the repos you want in a `grm.yml` manifest, and
`grm sync` clones the missing ones, puts each in a predictable place, and sets
the right git identity (name, email, signing key) in each one. `grm status`
shows where your disk differs from the manifest.

```yaml
# ~/.config/grm/grm.yml
root: ~/src
defaults:
  identity:
    name: Jane Doe
    email: jane@example.com
repos:
  - rust-lang/cargo                 # -> ~/src/github.com/rust-lang/cargo
  - url: git@gitlab.com:work/api.git
    identity:
      email: jane@work.example      # work email for this repo only
```

```sh
grm sync     # clone what's missing and fix git config
grm status   # show differences (exit code 1 if anything differs)
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
grm -f ~/.config/grm/grm.yml init --root ~/src
grm import --depth 3 --dry-run   # preview what would be added
grm import --depth 3
grm status
```

Without `-f`, `grm init` writes `./grm.yml` in the current directory. That
manifest is only found when you run `grm` from that directory or a directory
under it (see [Manifest discovery](#manifest-discovery)). Use the global
location if you want it to work from anywhere.

`--depth 3` is needed to find repos laid out as `{root}/{host}/{owner}/{name}`,
which is grm's default layout. The default depth is 2, which suits
`{root}/{owner}/{name}`.

From then on, use `grm add owner/name` instead of `git clone` so the manifest
stays up to date.

## Manifest reference

A complete, commented example is in [`grm.example.yml`](grm.example.yml).

### `root` (required)

The directory repos are cloned under. It must be an absolute path or start
with `~`. Relative roots are rejected.

```yaml
root: ~/src
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

A bare `name` with no owner is an error. A trailing `.git` or `/` is ignored
when working out the name. For nested groups, such as GitLab's
`group/sub/name`, everything between the host and the last segment counts as
the owner.

**Full form:** a mapping with these fields.

| Field | Required | Meaning |
| --- | --- | --- |
| `url` | yes | Any spec from the table above, not only full URLs. |
| `path` | no | Where to put the checkout. A relative path is resolved against `root`. Absolute paths and `~` paths are used as given. Default: `{root}/{host}/{owner}/{name}`. |
| `branch` | no | The branch to check out when the repo is first cloned. |
| `identity` | no | Overrides merged over `defaults.identity`. |

```yaml
repos:
  - rust-lang/cargo
  - url: git@gitlab.com:work/api.git
    path: work/api          # -> ~/src/work/api
    branch: main
    identity:
      email: jane@work.example
```

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
3. `~/.config/grm/grm.yml` (your platform's config directory; see
   [`dirs::config_dir`](https://docs.rs/dirs/latest/dirs/fn.config_dir.html)).
   On macOS that is `~/Library/Application Support/grm/grm.yml`.

If none of these exist, grm stops with an error and suggests `grm init`.

## Command reference

Global option: `-f, --file <PATH>` (or `GRM_FILE`) selects the manifest. It
can go before or after the subcommand. Every command exits with code 1 on an
error.

### `grm init`

Create a new manifest. It is written to `-f`/`GRM_FILE` if given, otherwise to
`./grm.yml`, with `root: ~/src` (or `--root`), the default settings and no
repos.

| Flag | Meaning |
| --- | --- |
| `--root <DIR>` | The root to store in the manifest. Default `~/src`. |
| `--force` | Overwrite an existing manifest. |

```sh
grm init
grm -f ~/.config/grm/grm.yml init --root ~/code
```

### `grm add <SPEC>`

Add a repo to the manifest, then clone it and apply its identity. `SPEC` can
be any form from the [repos](#repos) table. Adding a repo that is already
listed is an error. If the target path already exists, grm doesn't clone and
just applies the identity.

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

Remove a repo from the manifest. By default the checkout stays on disk, where
it becomes an "extra" that `grm prune` would delete.

| Flag | Meaning |
| --- | --- |
| `--delete` | Also delete the checkout. |
| `--force` | Delete even if the repo has uncommitted changes, unpushed commits or stashes. |
| `-y, --yes` | Don't ask for confirmation. |

```sh
grm remove rust-lang/cargo
grm remove rust-lang/cargo --delete
```

### `grm sync`

Make the disk match the manifest:

- Missing repos are cloned in parallel, then their identity is applied.
- Wrong or extra identity config keys are set or unset.
- A wrong or missing `origin` remote is changed to the manifest URL.
- Uncommitted changes and ahead/behind commits are only reported. grm never
  commits, pulls, pushes or resets.

| Flag | Meaning |
| --- | --- |
| `--dry-run` | Print the planned actions without changing anything. |
| `-j, --jobs <N>` | The number of parallel clones. Default 4. |
| `--prune` | Afterwards, also delete extras, as `grm prune` does. |
| `-y, --yes` | Don't ask for confirmation (for example when pruning). |

Exits with code 1 if any repo failed.

```sh
grm sync --dry-run
grm sync -j 8
grm sync --prune
```

### `grm status` (alias `grm diff`)

Show how the disk differs from the manifest. This doesn't change anything and
doesn't fetch. For each repo it reports one of:

- **missing**: not cloned yet.
- **ok**: matches the manifest.
- **drifted**: wrong identity config or a wrong `origin`. Uncommitted changes
  and ahead/behind counts are listed here too, for information only.

It also lists **extras**: git repos under `root` (up to 3 levels deep) that
aren't in the manifest.

**Exit code:** 0 if everything matches. 1 if any repo is missing or drifted,
or if there are extras. This makes it useful in scripts and CI:

```sh
grm status || echo "repos out of sync"
grm diff
```

### `grm prune`

Delete the extras: repos under `root` that aren't in the manifest. grm lists
them and asks for confirmation first. Repos with uncommitted changes, unpushed
commits or stashes are skipped with a warning unless you pass `--force`.

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
add them to the manifest. Repos without an `origin` remote and repos that are
already listed are skipped. Repos at the default `{root}/{host}/{owner}/{name}`
path are added in the short form. Others are added in the full form with
`path`. Nothing is cloned or deleted.

| Flag | Meaning |
| --- | --- |
| `--depth <N>` | How many directory levels to search. Default 2. Use 3 for the default `host/owner/name` layout. |
| `--dry-run` | Print what would be added without saving. |

```sh
grm import --depth 3 --dry-run
grm import ~/old-projects
```

## Safety

grm is built so that it can't lose your work by accident:

- **Unsaved work is never deleted without `--force`.** `grm prune`,
  `grm sync --prune` and `grm remove --delete` refuse to delete a repo with
  uncommitted changes or untracked files, stashes, or unpushed commits. A
  local branch with no upstream counts as unpushed.
- **Deleting always asks first.** Every deletion asks for confirmation
  (`[y/N]`, where the default is no) unless you pass `-y/--yes`.
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

## Learning and contributing

grm is also a project for learning Rust, and the code is commented with that
in mind. [LEARNING.md](LEARNING.md) (coming soon) walks through how it works.
See [CONTRIBUTING.md](CONTRIBUTING.md) for the code layout, conventions and
checks to run before committing.
