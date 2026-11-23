# Learning Rust with grm

grm is small (about 4,500 lines, half of them tests), does real work (it parses YAML,
runs `git`, walks directories and uses threads), and is commented for people
who know other languages but are new to Rust. This guide is a tour of it:

1. [Setup](#1-setup): build, run and test the code.
2. [Reading order](#2-reading-order): which file to read next, what it does,
   and the Rust it shows you.
3. [Concept index](#3-concept-index): look up a Rust concept and jump to where
   grm uses it.
4. [Exercises](#4-exercises): changes to make yourself, from easy to hard.

Throughout the code, comments that start with `// RUST:` explain a language
feature the first time a file uses it. To list them all:

```sh
grep -rn '// RUST:' src tests
```

Links like [src/spec.rs:76](src/spec.rs#L76) open the file at that line on
GitHub and in most editors' Markdown previews. Line numbers drift as the code
changes; if a link lands a few lines off, look nearby.

## 1. Setup

### Toolchain

The Rust version is pinned in [`mise.toml`](mise.toml). With
[mise](https://mise.jdx.dev) installed, run this once in the repo:

```sh
mise install        # installs rustc, cargo, clippy and rustfmt
```

If `cargo` isn't on your `PATH` afterwards, prefix commands with
`mise exec --`, for example `mise exec -- cargo test`. (If mise asks you to
trust the config file, run `mise trust`.)

### The everyday commands

| Command | What it does |
| --- | --- |
| `cargo build` | Compile into `target/debug/grm`. |
| `cargo run -- <args>` | Build, then run grm. Everything after `--` goes to grm, not cargo. |
| `cargo test` | Run every unit test (in `src/`) and integration test (in `tests/`). |
| `cargo test resolve` | Run only tests whose name contains `resolve`. |
| `cargo test -- --nocapture` | Show `println!` output from tests, which is normally hidden. |
| `cargo clippy --all-targets -- -D warnings` | The linter. It often suggests a more idiomatic way to write something; read its explanations. |
| `cargo fmt` | Format the code. `cargo fmt --check` only reports. |
| `cargo doc --open` | Build HTML docs from the `///` and `//!` comments and open them. Add `--document-private-items` to see the private functions too. |

`fmt`, `clippy` and `test` must all pass before a commit (see
[CONTRIBUTING.md](CONTRIBUTING.md)).

### Running grm safely while you experiment

Read-only commands are always safe. You can point them at the example
manifest:

```sh
cargo run -- status -f grm.example.yml     # everything "missing"; extras from your real ~/workspace
cargo run -- sync --dry-run -f grm.example.yml
cargo run -- --help
cargo run -- add --help
```

For commands that change things (`sync`, `add`, `prune`, ...) use a throwaway
manifest and root, so nothing touches your real repos:

```sh
mkdir -p /tmp/grm-play
cargo run -- -f /tmp/grm-play/grm.yml init --root /tmp/grm-play/repos
cargo run -- -f /tmp/grm-play/grm.yml add https://github.com/rust-lang/rustlings
cargo run -- -f /tmp/grm-play/grm.yml status
```

Or `export GRM_FILE=/tmp/grm-play/grm.yml` once and drop the `-f`.

### Reading compiler errors

Rust's compiler errors are long but good. Read the whole message, especially
the `help:` lines, which often contain the fix. `rustc --explain E0382` (with
the code from the error) prints a longer explanation with examples.

## 2. Reading order

The files are listed in the order a command flows through them: entry point,
then the CLI definition, then the data model, then the git layer, then drift
detection, then the commands that put it together. Each section says what the
file does and the Rust concepts to look for.

### [`src/main.rs`](src/main.rs): the entry point

Parses the command line into a `Cli` value, builds a `Context`, and calls the
`run` function of the chosen subcommand. Any error is printed as
`error: ...` and becomes exit code 1.

- Modules: `mod cli;` pulls in `src/cli.rs`
  ([main.rs:6](src/main.rs#L6)).
- `main` returning an `ExitCode` ([main.rs:22](src/main.rs#L22)).
- An exhaustive `match` on an enum: forget a subcommand and it won't compile
  ([main.rs:34](src/main.rs#L34)).
- `Result` and printing an `anyhow` error chain with `{:#}`
  ([main.rs:46](src/main.rs#L46), [main.rs:51](src/main.rs#L51)).

### [`src/cli.rs`](src/cli.rs): the command-line interface

Only describes the CLI; clap generates the parser from the structs. The `///`
comments become the `--help` text.

- `use` ([cli.rs:6](src/cli.rs#L6)).
- `#[derive(...)]`: the compiler writes trait implementations for you
  ([cli.rs:12](src/cli.rs#L12)).
- `Option<T>` instead of null ([cli.rs:20](src/cli.rs#L20)).
- Enums whose variants carry data ([cli.rs:30](src/cli.rs#L30)).
- `usize` ([cli.rs:121](src/cli.rs#L121)).
- Unit tests: `#[cfg(test)]`, `use super::*`, `#[test]`
  ([cli.rs:166](src/cli.rs#L166)).

### [`src/commands/mod.rs`](src/commands/mod.rs): shared command helpers

Declares the subcommand modules and holds what they share: `Context` (the
global `-f` path) and `confirm`, the `[y/N]` prompt.

- `pub mod` declarations ([commands/mod.rs:6](src/commands/mod.rs#L6)).
- `String::new()`, `&mut` borrows, and `?` with `.context(...)`
  ([commands/mod.rs:40](src/commands/mod.rs#L40)).
- `matches!` ([commands/mod.rs:54](src/commands/mod.rs#L54)).
- Splitting a pure function (`is_yes`) out of an I/O function (`confirm`) so
  it can be tested.

### [`src/manifest.rs`](src/manifest.rs): the `grm.yml` data model

The types that mirror the YAML file, plus: finding the manifest (`discover`),
reading and writing it (`load`, `save`), and `Manifest::resolve`, which turns
each entry into a `ResolvedRepo` with an absolute path and a merged identity.
Most commands start with `discover` → `load` → `resolve`.

- serde attributes: `default` ([manifest.rs:46](src/manifest.rs#L46)),
  `skip_serializing_if` ([manifest.rs:63](src/manifest.rs#L63)),
  `untagged` enums ([manifest.rs:120](src/manifest.rs#L120)).
- Implementing a trait by hand: `impl Default for Defaults`
  ([manifest.rs:74](src/manifest.rs#L74)).
- `.ok()` to turn a `Result` into an `Option`
  ([manifest.rs:173](src/manifest.rs#L173)).
- Iterating with `.ancestors()` ([manifest.rs:183](src/manifest.rs#L183)).
- `with_context(|| ...)` ([manifest.rs:207](src/manifest.rs#L207)).
- `impl` blocks and `&self` methods ([manifest.rs:258](src/manifest.rs#L258)).
- `HashMap` ([manifest.rs:289](src/manifest.rs#L289)).
- Destructuring enum variants in a `match`
  ([manifest.rs:295](src/manifest.rs#L295)).
- `Option` combinators: `or_else`, `map`, `unwrap_or_default`
  ([manifest.rs:353](src/manifest.rs#L353)).
- Tests using `tempfile::tempdir()` for real files on disk (bottom of the
  file).

### [`src/spec.rs`](src/spec.rs): parsing `owner/name` and URLs

Turns what you type (`rust-lang/cargo`, `git@github.com:foo/bar.git`, ...)
into a `RepoSpec`, and `normalize_url` makes URLs comparable regardless of
protocol. Pure string handling, no I/O, so it's a good file to experiment in:
change something and run `cargo test spec`.

- `String` vs `&str` ([spec.rs:31](src/spec.rs#L31)).
- `#[serde(rename_all = "lowercase")]` and `#[default]` on enum variants
  ([spec.rs:49](src/spec.rs#L49)).
- `if let Some(x) = ...` and `?` ([spec.rs:82](src/spec.rs#L82)).
- `.split(...).collect()` into a `Vec<&str>` ([spec.rs:99](src/spec.rs#L99)).
- Slice patterns: `[owner, name]`, `[host, middle @ .., name]`
  ([spec.rs:104](src/spec.rs#L104)).
- Matching nested types: `Option<Result<..>>`
  ([spec.rs:146](src/spec.rs#L146)).
- `let ... else` ([spec.rs:196](src/spec.rs#L196)).
- Lifetime elision: returning a `&str` that borrows from the input
  ([spec.rs:247](src/spec.rs#L247)).
- Closures stored in a variable ([spec.rs:264](src/spec.rs#L264)).
- Table-style tests looping over many inputs (`junk_is_rejected`).

### [`src/git.rs`](src/git.rs): talking to git

Every git operation is a function here that runs the `git` program and
returns a `Result`. It also has `find_repos` (a recursive directory walk) and
`unsaved_work`, the one safety check that `prune` and `remove --delete` share.

- `anyhow` imports: `bail!` and `Context` ([git.rs:14](src/git.rs#L14)).
- `std::process::Command`, a builder ([git.rs:23](src/git.rs#L23)).
- Lazily built error messages ([git.rs:34](src/git.rs#L34)).
- Bytes to text: `String::from_utf8_lossy` ([git.rs:51](src/git.rs#L51)).
- `.output()` (capture) vs `.status()` (inherit the terminal) in `clone`
  ([git.rs:66](src/git.rs#L66)).
- `match` on `Option<i32>` exit codes ([git.rs:104](src/git.rs#L104)).
- Parsing numbers with `parse::<u64>()` ([git.rs:210](src/git.rs#L210)).
- Iterators: `.next()`, `.and_then`, tuple `match`
  ([git.rs:251](src/git.rs#L251)).
- Passing `&mut Vec` into a recursive helper ([git.rs:275](src/git.rs#L275)).
- `&'static str` ([git.rs:328](src/git.rs#L328)).
- Tests that build real repos in temp directories, isolated from your own git
  config with `GIT_CONFIG_GLOBAL=/dev/null`.

### [`src/state.rs`](src/state.rs): drift detection

Compares the manifest with the disk without changing anything. `inspect`
returns a `Report`: one `RepoState` (`Missing`, `Ok` or `Drifted(Vec<Drift>)`)
per repo, plus the extras. `status` prints it, `sync` fixes it and `prune`
deletes its extras.

- Enums as a data model: `Drift`, `RepoState`
  ([state.rs:26](src/state.rs#L26)).
- Methods that answer questions about the data: `is_informational`,
  `needs_sync`, `has_drift` ([state.rs:46](src/state.rs#L46)).
- Passing a method as a closure: `.any(RepoReport::needs_sync)`
  ([state.rs:109](src/state.rs#L109)).
- Moving values out of a `Vec` with `for x in vec`
  ([state.rs:125](src/state.rs#L125)).
- Collecting an iterator of `Result`s into one `Result`
  ([state.rs:143](src/state.rs#L143)).
- `Option::is_some_and` ([state.rs:175](src/state.rs#L175)).
- `Path::starts_with` compares path components
  ([state.rs:220](src/state.rs#L220)).

### [`src/commands/*.rs`](src/commands): the subcommands

Each file has `pub fn run(args, &Context) -> Result<ExitCode>`. Read them in
this order:

1. [`init.rs`](src/commands/init.rs): the simplest. `match` on an `Option`
   ([init.rs:27](src/commands/init.rs#L27)), `as_deref` + `unwrap_or`
   ([init.rs:40](src/commands/init.rs#L40)), struct update syntax
   `..Defaults::default()` ([init.rs:86](src/commands/init.rs#L86)),
   `.ok()?` ([init.rs:122](src/commands/init.rs#L122)), `bool::then_some`
   ([init.rs:129](src/commands/init.rs#L129)).
2. [`status.rs`](src/commands/status.rs): prints a `Report`. Importing a
   trait to get its methods ([status.rs:16](src/commands/status.rs#L16)),
   returning `ExitCode::from(1)` ([status.rs:26](src/commands/status.rs#L26)),
   `writeln!` into a `String` ([status.rs:45](src/commands/status.rs#L45)),
   `pub(crate)` ([status.rs:126](src/commands/status.rs#L126)),
   `impl Display` arguments ([status.rs:175](src/commands/status.rs#L175)).
   Note how `render` returns a `String` instead of printing, so it can be
   tested.
3. [`add.rs`](src/commands/add.rs): ownership in signatures
   ([add.rs:67](src/commands/add.rs#L67)), `.iter().find(...)`
   ([add.rs:79](src/commands/add.rs#L79)), `.pop()` + `.context`
   ([add.rs:135](src/commands/add.rs#L135)), matching a tuple of two
   `Option`s ([add.rs:187](src/commands/add.rs#L187)).
4. [`remove.rs`](src/commands/remove.rs): `if let Ok(...)`
   ([remove.rs:88](src/commands/remove.rs#L88)), `.position`
   ([remove.rs:92](src/commands/remove.rs#L92)), an
   `enumerate`/`filter`/`map`/`collect` chain
   ([remove.rs:103](src/commands/remove.rs#L103)), and a slice-pattern
   `match` on the results ([remove.rs:113](src/commands/remove.rs#L113)).
5. [`prune.rs`](src/commands/prune.rs): a small options struct deriving
   `Copy` and `Default` ([prune.rs:28](src/commands/prune.rs#L28)) and
   `prune_extras`, shared with `sync --prune`
   ([prune.rs:60](src/commands/prune.rs#L60)).
6. [`import.rs`](src/commands/import.rs): `into_iter` + `extend`
   ([import.rs:82](src/commands/import.rs#L82)), `HashSet`
   ([import.rs:103](src/commands/import.rs#L103),
   [import.rs:128](src/commands/import.rs#L128)), `map_or`
   ([import.rs:170](src/commands/import.rs#L170)). Planning (`plan_import`)
   is separate from doing, which makes `--dry-run` easy.
7. [`sync.rs`](src/commands/sync.rs): the most advanced. `&=`
   ([sync.rs:51](src/commands/sync.rs#L51)), `filter`/`map`
   ([sync.rs:108](src/commands/sync.rs#L108)), `if` as an expression
   ([sync.rs:125](src/commands/sync.rs#L125)), `type` aliases
   ([sync.rs:205](src/commands/sync.rs#L205)), atomics
   ([sync.rs:213](src/commands/sync.rs#L213)), scoped threads
   ([sync.rs:216](src/commands/sync.rs#L216)), `move` closures
   ([sync.rs:222](src/commands/sync.rs#L222)), `let ... else { break }`
   ([sync.rs:230](src/commands/sync.rs#L230)).

### [`tests/`](tests): end-to-end tests

[`tests/cli.rs`](tests/cli.rs) runs the real `grm` binary against bare git
repos in a temp directory (no network, no real home directory).
[`tests/common/mod.rs`](tests/common/mod.rs) builds that sandbox.

- What an integration test is ([tests/cli.rs:10](tests/cli.rs#L10)) and why
  `common` needs `#![allow(dead_code)]`
  ([tests/common/mod.rs:13](tests/common/mod.rs#L13)).
- `TempDir` and `Drop`: the sandbox deletes itself
  ([tests/common/mod.rs:25](tests/common/mod.rs#L25)).
- `assert_cmd`: run the binary and assert on its exit code
  ([tests/common/mod.rs:224](tests/common/mod.rs#L224),
  [tests/cli.rs:44](tests/cli.rs#L44)).

## 3. Concept index

| Concept | Where to see it |
| --- | --- |
| Ownership & borrowing (`&`, `&mut`, moves) | [add.rs:67](src/commands/add.rs#L67), [git.rs:275](src/git.rs#L275), [state.rs:125](src/state.rs#L125), [import.rs:82](src/commands/import.rs#L82), [sync.rs:222](src/commands/sync.rs#L222) |
| `&str` vs `String` | [spec.rs:31](src/spec.rs#L31), [commands/mod.rs:40](src/commands/mod.rs#L40), [spec.rs:99](src/spec.rs#L99) |
| `Option`, `Result` and `?` | [cli.rs:20](src/cli.rs#L20), [main.rs:46](src/main.rs#L46), [spec.rs:82](src/spec.rs#L82), [git.rs:42](src/git.rs#L42), [init.rs:122](src/commands/init.rs#L122), [manifest.rs:173](src/manifest.rs#L173) |
| Enums & `match` | [cli.rs:30](src/cli.rs#L30), [main.rs:34](src/main.rs#L34), [state.rs:26](src/state.rs#L26), [git.rs:104](src/git.rs#L104), [manifest.rs:295](src/manifest.rs#L295) |
| Tuple matching | [add.rs:187](src/commands/add.rs#L187), [git.rs:256](src/git.rs#L256) |
| Slice patterns, `if let`, `let else`, `matches!` | [spec.rs:104](src/spec.rs#L104), [spec.rs:196](src/spec.rs#L196), [remove.rs:113](src/commands/remove.rs#L113), [commands/mod.rs:54](src/commands/mod.rs#L54) |
| Structs & `impl` blocks | [manifest.rs:258](src/manifest.rs#L258), [state.rs:77](src/state.rs#L77), [status.rs:157](src/commands/status.rs#L157), [init.rs:86](src/commands/init.rs#L86) (struct update) |
| Traits & `derive` | [cli.rs:12](src/cli.rs#L12), [manifest.rs:74](src/manifest.rs#L74) (hand-written `impl Default`), [status.rs:16](src/commands/status.rs#L16) (extension trait), [status.rs:175](src/commands/status.rs#L175) (`impl Display`), [manifest.rs:30](src/manifest.rs#L30) (`Context as _`) |
| serde attributes | [manifest.rs:46](src/manifest.rs#L46) (`default`), [manifest.rs:63](src/manifest.rs#L63) (`skip_serializing_if`), [manifest.rs:120](src/manifest.rs#L120) (`untagged`), [spec.rs:49](src/spec.rs#L49) (`rename_all`) |
| Iterators & closures | [spec.rs:264](src/spec.rs#L264), [state.rs:109](src/state.rs#L109), [state.rs:143](src/state.rs#L143), [remove.rs:103](src/commands/remove.rs#L103), [sync.rs:108](src/commands/sync.rs#L108), [import.rs:103](src/commands/import.rs#L103) |
| Modules & visibility (`mod`, `pub`, `pub(crate)`) | [main.rs:6](src/main.rs#L6), [commands/mod.rs:6](src/commands/mod.rs#L6), [status.rs:126](src/commands/status.rs#L126), [cli.rs:169](src/cli.rs#L169) (`super::*`) |
| Lifetimes (`'static`, elision) | [git.rs:328](src/git.rs#L328), [spec.rs:247](src/spec.rs#L247), [remove.rs:138](src/commands/remove.rs#L138) |
| `std::process::Command` | [git.rs:23](src/git.rs#L23), [git.rs:66](src/git.rs#L66) (`.status()` vs `.output()`), [init.rs:117](src/commands/init.rs#L117) |
| Scoped threads (`thread::scope`) & atomics | [sync.rs:216](src/commands/sync.rs#L216), [sync.rs:213](src/commands/sync.rs#L213) |
| `anyhow` errors & context | [git.rs:14](src/git.rs#L14), [manifest.rs:207](src/manifest.rs#L207), [commands/mod.rs:42](src/commands/mod.rs#L42), [main.rs:51](src/main.rs#L51) |
| `ExitCode` | [main.rs:22](src/main.rs#L22), [status.rs:26](src/commands/status.rs#L26), [sync.rs:55](src/commands/sync.rs#L55) |
| Unit vs integration tests | [cli.rs:166](src/cli.rs#L166), [cli.rs:173](src/cli.rs#L173), [tests/cli.rs:10](tests/cli.rs#L10), [tests/common/mod.rs:13](tests/common/mod.rs#L13) |
| `tempfile` | [tests/common/mod.rs:25](tests/common/mod.rs#L25), [tests/common/mod.rs:45](tests/common/mod.rs#L45), [git.rs:393](src/git.rs#L393) |
| Collections (`Vec`, `HashMap`, `HashSet`) | [git.rs:268](src/git.rs#L268), [manifest.rs:289](src/manifest.rs#L289), [import.rs:103](src/commands/import.rs#L103) |
| Macros (`format!`, `bail!`, `matches!`, `writeln!`) | [git.rs:14](src/git.rs#L14), [spec.rs:191](src/spec.rs#L191), [status.rs:45](src/commands/status.rs#L45), [tests/common/mod.rs:224](tests/common/mod.rs#L224) |

## 4. Exercises

Roughly easiest first. Each one says which files to touch; the existing tests
in those files show how to test your change. Run `cargo fmt`,
`cargo clippy --all-targets -- -D warnings` and `cargo test` when you're done.

### 1. A clearer message for "path exists but is not a git repo" (easy)

If a repo's path exists but has no `.git`, `status` calls it "missing" and
`sync` fails with git's "destination path already exists" error. Add a new
`RepoState` variant (for example `NotARepo`) and report it properly.

- [`src/state.rs`](src/state.rs): `inspect_repo` and `is_git_repo`; add the
  variant and decide whether `needs_sync` / `has_drift` count it.
- The compiler will now point at every `match` on `RepoState` that doesn't
  handle the new variant ([status.rs](src/commands/status.rs),
  [sync.rs](src/commands/sync.rs)). That's exhaustive `match` doing its job.
- Test: a unit test in `state.rs`, plus an integration test in
  [`tests/cli.rs`](tests/cli.rs) that creates a plain directory at
  `fx.repo_path("alpha")`.

### 2. `grm sync --prune --force` (easy)

`sync --prune` never deletes repos with unsaved work. Add a `--force` flag to
`sync` that is passed through to pruning.

- [`src/cli.rs`](src/cli.rs): add the field to `SyncArgs`. Consider
  `#[arg(long, requires = "prune")]` so `--force` without `--prune` is a
  usage error.
- [`src/commands/sync.rs`](src/commands/sync.rs): set `force` in
  `PruneOptions` instead of `false`.
- Update the module doc comment at the top of `sync.rs` and the README.
- Test: copy `prune_force_removes_dirty_and_unpushed_extras` in
  [`tests/cli.rs`](tests/cli.rs).

### 3. Quieter parallel clone output (medium)

With `-j 4`, four `git clone` progress bars write to the terminal at once.
Make clones quiet and print one line per repo as each finishes.

- [`src/git.rs`](src/git.rs): `clone` uses `.status()`, which shares the
  terminal. Switch to `.output()` (captured) and pass `--quiet`, and put
  git's stderr into the error message on failure, like `git()` does.
- [`src/commands/sync.rs`](src/commands/sync.rs): print from inside the
  worker closure in `clone_all`. Is `println!` from several threads safe?
  (Yes: each call locks stdout. Why might lines still come out in a
  different order each run?)
- Bonus: keep progress output when `-j 1`.

### 4. `grm status --json` (medium)

Print the report as JSON for scripts.

- `cargo add serde_json`.
- [`src/cli.rs`](src/cli.rs): add `#[arg(long)] json: bool` to `StatusArgs`.
- [`src/state.rs`](src/state.rs): add `Serialize` to the `#[derive(...)]` of
  `Report`, `RepoReport`, `RepoState` and `Drift`. The compiler will tell you
  `ResolvedRepo` and `RepoSpec` need it too
  ([manifest.rs](src/manifest.rs), [spec.rs](src/spec.rs)).
- Look at serde's `#[serde(tag = "state")]` and
  `#[serde(rename_all = "snake_case")]` to get JSON shaped the way you want.
- [`src/commands/status.rs`](src/commands/status.rs): if `args.json`, print
  `serde_json::to_string_pretty(&report)?` instead of `render`. Keep the
  same exit code.
- Test: parse the output in an integration test with
  `serde_json::from_slice::<serde_json::Value>`.

### 5. `grm exec <cmd>...` across repos (medium)

Run a command in every cloned repo, for example `grm exec git fetch`.

- [`src/cli.rs`](src/cli.rs): a new `Exec(ExecArgs)` variant. Use
  `#[arg(trailing_var_arg = true, required = true)] cmd: Vec<String>` so
  flags after the command name go to it, not to grm.
- [`src/main.rs`](src/main.rs): the compiler will make you add the `match`
  arm.
- New file `src/commands/exec.rs`, declared in
  [`src/commands/mod.rs`](src/commands/mod.rs): `discover` → `load` →
  `resolve`, skip repos that aren't cloned, and run
  `std::process::Command::new(&cmd[0]).args(&cmd[1..]).current_dir(&repo.path)`.
- Print a header per repo; exit 1 if any command failed.
- Bonus: a `-j` flag reusing the `thread::scope` pattern from `sync.rs`.

### 6. Show which branch is checked out (medium)

Add the current branch to `status` output, and warn when it differs from the
manifest's `branch`.

- [`src/git.rs`](src/git.rs): `current_branch(repo) -> Result<Option<String>>`
  using `git symbolic-ref --quiet --short HEAD` (non-zero exit means detached
  HEAD; look at how `config_get` handles exit codes).
- [`src/state.rs`](src/state.rs): a new informational `Drift` variant.
- [`src/commands/status.rs`](src/commands/status.rs): `describe_drift`.

### 7. Parallel `status` (harder)

`state::inspect` checks repos one after another, running several git
commands each. With many repos, that's slow.

- [`src/state.rs`](src/state.rs): inspect repos in parallel with
  `std::thread::scope`, keeping the results in manifest order.
- Look at `clone_all` in [`sync.rs`](src/commands/sync.rs) for a pattern; can
  you write a generic `fn parallel_map<T, R>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R>`
  and use it in both places? The trait bounds (`Sync`, `Send`) are the
  interesting part.

### 8. Keep comments when editing `grm.yml` (hard)

`add`, `remove` and `import` load the YAML into structs and write it back,
which loses comments and formatting. Make them edit the text instead.

- [`src/manifest.rs`](src/manifest.rs): `save` serializes the whole
  `Manifest`. Instead, add functions that append or remove one entry in the
  original text, for example by finding the `repos:` block and inserting a
  line, then re-parsing with `serde_yaml_ng` to check the result is still a
  valid manifest.
- Alternatively, look for a YAML crate that preserves formatting and compare
  the trade-offs.
- [`src/commands/add.rs`](src/commands/add.rs),
  [`remove.rs`](src/commands/remove.rs),
  [`import.rs`](src/commands/import.rs): call the new functions.
- Test with a manifest full of comments and odd indentation, and update the
  "Known limitations" section of the README.
