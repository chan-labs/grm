//! `grm status`: show drift between manifest and disk (alias: `diff`).
//!
//! - `state::inspect` the manifest and print each repo with its state
//!   (coloured with `owo-colors`), then extras, then a summary line.
//! - Exit code: `ExitCode::from(1)` if `Report::has_drift()`, else success.
//!   Uncommitted changes and ahead/behind counts are shown but are
//!   informational only, so on their own they don't cause exit code 1
//!   (see `Drift::is_informational`).
//!
//! Also home to [`Painter`], the small colour helper `sync` and `prune` share.

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{Context as _, Result};
// RUST: `OwoColorize` is a *trait*: importing it adds methods like
// `.style(...)` to every displayable value (strings, numbers, ...).
use owo_colors::{OwoColorize, Style};

use crate::cli::StatusArgs;
use crate::commands::Context;
use crate::manifest;
use crate::state::{self, Drift, RepoState, Report};

/// Run `grm status`. Returns `ExitCode::SUCCESS` unless documented otherwise.
pub fn run(_args: StatusArgs, ctx: &Context) -> Result<ExitCode> {
    // RUST: `as_deref()` turns `Option<PathBuf>` into `Option<&Path>`
    // (borrowing instead of moving).
    let path = manifest::discover(ctx.manifest_path.as_deref())?;
    let m = manifest::load(&path)?;
    let report = state::inspect(&m).context("inspecting repos")?;

    let p = Painter::new();
    print!("{}", render(&report, &p));

    Ok(if report.has_drift() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Build the whole status output as a string (easy to unit-test).
fn render(report: &Report, p: &Painter) -> String {
    // RUST: `String` implements `std::fmt::Write`, so `writeln!` can append
    // formatted text to it just like `println!` prints to stdout. Writing to
    // a String can't fail, which is why we ignore the returned `Result`
    // with `let _ =`.
    use std::fmt::Write as _;
    let mut out = String::new();

    let (mut ok, mut missing, mut drifted) = (0, 0, 0);
    for r in &report.repos {
        let name = &r.repo.spec.name;
        let path = r.repo.path.display();
        match &r.state {
            RepoState::Ok => {
                ok += 1;
                let _ = writeln!(out, "{} {name}  {}", p.ok("✓"), p.dim(&path));
            }
            RepoState::Missing => {
                missing += 1;
                let _ = writeln!(
                    out,
                    "{} {name} {}  {}",
                    p.err("✗"),
                    p.err("missing"),
                    p.dim(&path)
                );
            }
            RepoState::Drifted(drifts) => {
                // Only informational drift → the repo still counts as ok.
                if r.needs_sync() {
                    drifted += 1;
                    let _ = writeln!(out, "{} {name}  {}", p.warn("~"), p.dim(&path));
                } else {
                    ok += 1;
                    let _ = writeln!(out, "{} {name}  {}", p.ok("✓"), p.dim(&path));
                }
                for d in drifts {
                    let line = describe_drift(d);
                    let line = if d.is_informational() {
                        p.dim(&line)
                    } else {
                        line
                    };
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
    }

    if !report.extras.is_empty() {
        let _ = writeln!(out, "\n{}", p.bold("extra repos (not in manifest):"));
        for e in &report.extras {
            let _ = writeln!(out, "  {} {}", p.warn("?"), e.display());
        }
    }

    let _ = writeln!(
        out,
        "\n{ok} ok, {missing} missing, {drifted} drifted, {} extra",
        report.extras.len()
    );
    out
}

/// One human-readable line for a drift, e.g. `user.email: a → b`.
/// Shared with `sync` for its dry-run plan.
pub(crate) fn describe_drift(d: &Drift) -> String {
    match d {
        Drift::Config {
            key,
            actual,
            expected,
        } => format!("{key}: {} → {}", show(actual), show(expected)),
        Drift::Remote { actual, expected } => {
            format!("origin: {} → {expected}", show(actual))
        }
        Drift::Dirty => "uncommitted changes".to_string(),
        Drift::AheadBehind { ahead, behind } => format!("ahead {ahead}, behind {behind}"),
    }
}

/// `Some(v)` → `v`, `None` → `(unset)`.
fn show(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("(unset)")
}

/// Applies colours only when stdout is a terminal and `NO_COLOR` isn't set
/// (see <https://no-color.org>), so piped output stays plain text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Painter {
    enabled: bool,
}

impl Painter {
    /// Decide once, from the environment, whether to colour output.
    pub(crate) fn new() -> Self {
        // RUST: `std::env::var_os` returns `Option<OsString>`; `is_some_and`
        // checks a condition on the value only if there is one.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Painter {
            enabled: !no_color && std::io::stdout().is_terminal(),
        }
    }

    /// A painter that never colours (for tests).
    #[cfg(test)]
    pub(crate) fn plain() -> Self {
        Painter { enabled: false }
    }

    /// Render `text` with `style` if colours are enabled.
    // RUST: `impl std::fmt::Display` accepts any type that can be printed
    // with `{}` (strings, paths' `.display()`, numbers, ...).
    fn paint(&self, text: impl std::fmt::Display, style: Style) -> String {
        if self.enabled {
            text.style(style).to_string()
        } else {
            text.to_string()
        }
    }

    pub(crate) fn ok(&self, t: impl std::fmt::Display) -> String {
        self.paint(t, Style::new().green().bold())
    }
    pub(crate) fn warn(&self, t: impl std::fmt::Display) -> String {
        self.paint(t, Style::new().yellow().bold())
    }
    pub(crate) fn err(&self, t: impl std::fmt::Display) -> String {
        self.paint(t, Style::new().red().bold())
    }
    pub(crate) fn dim(&self, t: impl std::fmt::Display) -> String {
        self.paint(t, Style::new().dimmed())
    }
    pub(crate) fn bold(&self, t: impl std::fmt::Display) -> String {
        self.paint(t, Style::new().bold())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Identity, ResolvedRepo};
    use crate::spec::RepoSpec;
    use crate::state::RepoReport;
    use std::path::PathBuf;

    fn repo(name: &str, state: RepoState) -> RepoReport {
        RepoReport {
            repo: ResolvedRepo {
                spec: RepoSpec {
                    host: "github.com".into(),
                    owner: "o".into(),
                    name: name.into(),
                    url: format!("git@github.com:o/{name}.git"),
                },
                path: PathBuf::from(format!("/src/{name}")),
                branch: None,
                identity: Identity::default(),
            },
            state,
        }
    }

    #[test]
    fn renders_every_state_plainly() {
        let report = Report {
            repos: vec![
                repo("a", RepoState::Ok),
                repo("b", RepoState::Missing),
                repo(
                    "c",
                    RepoState::Drifted(vec![
                        Drift::Config {
                            key: "user.email".into(),
                            actual: None,
                            expected: Some("x@y".into()),
                        },
                        Drift::AheadBehind {
                            ahead: 2,
                            behind: 1,
                        },
                    ]),
                ),
                repo("d", RepoState::Drifted(vec![Drift::Dirty])),
            ],
            extras: vec![PathBuf::from("/src/old")],
        };
        let out = render(&report, &Painter::plain());
        assert_eq!(
            out,
            "✓ a  /src/a\n\
             ✗ b missing  /src/b\n\
             ~ c  /src/c\n    user.email: (unset) → x@y\n    ahead 2, behind 1\n\
             ✓ d  /src/d\n    uncommitted changes\n\
             \nextra repos (not in manifest):\n  ? /src/old\n\
             \n2 ok, 1 missing, 1 drifted, 1 extra\n"
        );
    }
}
