//! Subcommand implementations plus small helpers they share.
//!
//! Every subcommand module exposes
//! `pub fn run(args: <XArgs>, ctx: &Context) -> anyhow::Result<ExitCode>`.

pub mod add;
pub mod import;
pub mod init;
pub mod prune;
pub mod remove;
pub mod status;
pub mod sync;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// Global state shared by every subcommand.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Manifest path from `-f/--file` or `GRM_FILE`. `None` means "discover it"
    /// with `manifest::discover`.
    // RUST: `#[allow(dead_code)]` silences the "never read" warning until the
    // stub commands use this. TODO: remove once commands are implemented.
    #[allow(dead_code)]
    pub manifest_path: Option<PathBuf>,
}

/// Print `prompt` followed by ` [y/N] ` and read an answer from stdin.
///
/// Returns `true` only for `y` / `yes` (case-insensitive). Anything else,
/// including an empty line or end-of-input, is `false`.
// TODO: remove the allow once `remove` / `prune` / `sync` call this.
#[allow(dead_code)]
pub fn confirm(prompt: &str) -> Result<bool> {
    // RUST: `print!` (no newline) does not flush stdout automatically, so we
    // flush ourselves to make sure the prompt is visible before we wait.
    print!("{prompt} [y/N] ");
    io::stdout().flush().context("flushing stdout")?;

    // RUST: `String::new()` makes an empty, growable, owned string.
    let mut answer = String::new();
    // RUST: `?` returns early with the error if the call failed; otherwise it
    // unwraps the `Ok` value. `.context(...)` (from anyhow) adds a message.
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .context("reading answer from stdin")?;

    Ok(is_yes(&answer))
}

/// Pure part of [`confirm`], split out so it can be unit-tested.
#[allow(dead_code)]
fn is_yes(answer: &str) -> bool {
    // RUST: `matches!` is a macro that returns true if the value fits a pattern.
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::is_yes;

    #[test]
    fn yes_answers() {
        assert!(is_yes("y\n"));
        assert!(is_yes(" YES "));
        assert!(!is_yes(""));
        assert!(!is_yes("n"));
        assert!(!is_yes("yeah"));
    }
}
