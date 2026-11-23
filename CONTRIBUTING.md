# Contributing to grm

`grm` is also a learning project: the owner is new to Rust, so the code is
written to be read and learned from. Please follow these conventions.

## Code layout

| File | Purpose |
| --- | --- |
| `src/main.rs` | Parse CLI, build `Context`, dispatch to a command |
| `src/cli.rs` | clap definitions (flags, subcommands, help text) |
| `src/spec.rs` | Parse `owner/name` / URLs into `RepoSpec`; URL normalization |
| `src/manifest.rs` | `grm.yml` types, load/save/discover, resolving entries |
| `src/git.rs` | Wrappers around the `git` CLI |
| `src/state.rs` | Drift detection (manifest vs. disk) |
| `src/commands/*.rs` | One file per subcommand, each with `pub fn run(args, &Context) -> Result<ExitCode>` |

## Conventions

- **Every file starts with a `//!` module doc** explaining what the module is for.
- **Explain Rust concepts at first use with `// RUST:` comments.** The first
  time a file uses a concept a beginner might not know (`?`, `Option`,
  lifetimes, traits, iterators, closures, `impl Trait`, macros, ...), add a
  short `// RUST: ...` comment saying what it does. Don't repeat the same
  explanation all over the file.
- **Errors:** return `anyhow::Result<T>` and add context with `.context("...")`
  / `.with_context(|| format!("... {}", path.display()))` so messages say what
  was being attempted.
- **No `unwrap()` / `expect()` outside tests.** Use `?` and context instead.
- **Public signatures are a contract.** The types and function signatures in
  `spec.rs`, `manifest.rs`, `git.rs`, `state.rs`, `commands/mod.rs` and the
  `run` functions are used by other modules developed in parallel. Don't
  change them; if you need a change, report it (in your PR / task report)
  instead. Adding private helpers is fine.
- **Stubs:** unimplemented functions are `todo!()` with doc comments describing
  the intended behaviour. When you implement a stub module, remove its
  `#![allow(dead_code, unused_variables)]` (or narrow it to what's still needed).
- **Tests:** unit tests go in a `#[cfg(test)] mod tests` at the bottom of the
  file; end-to-end CLI tests go in `tests/` using `assert_cmd`, `tempfile` and
  `predicates`. Tests must not touch the real home directory or network
  (use local bare repos created in a temp dir as remotes).

## Before committing

The Rust toolchain is pinned in `mise.toml`; prefix commands with
`mise exec --` if `cargo` isn't on your PATH.

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

All three must pass.
