//! Edit the `repos:` list in the text of a `grm.yml`, so that `add`, `remove`
//! and `import` keep the comments, blank lines and key order you wrote.
//!
//! This is deliberately simple: it understands the common block-style shape
//!
//! ```yaml
//! repos:            # or `repos: []`, or no `repos:` key at all
//!   - owner/name
//!   - url: ...
//!     path: ...
//! ```
//!
//! and returns `None` for anything else (flow-style `[a, b]` lists, odd
//! indentation, anchors spread over the list...). The caller then checks the
//! result by parsing it again, and falls back to rewriting the whole file if
//! the text edit was refused or would change the meaning.

use super::RepoEntry;

/// One change to the `repos:` list.
#[derive(Debug, Clone, Copy)]
pub enum RepoEdit<'a> {
    /// Add these entries after the last one.
    Append(&'a [RepoEntry]),
    /// Remove the entry at this index (0-based, in file order).
    Remove(usize),
}

/// Where the `repos:` list is in the file.
// RUST: an enum whose variants carry different data. `match` on it below
// forces every shape to be handled.
#[derive(Debug, PartialEq, Eq)]
enum Block {
    /// There is no top-level `repos:` key.
    Missing,
    /// `repos: []` on line `key`.
    EmptyFlow { key: usize },
    /// `repos:` on line `key`, then items. `indent` is the column of each
    /// item's `-`. Each item is a half-open range of lines `start..end`,
    /// where `end` is one past the item's last non-comment line.
    Seq {
        key: usize,
        indent: usize,
        items: Vec<(usize, usize)>,
    },
}

/// Apply `edit` to the manifest text `text`. Returns the new text, or `None`
/// if the file has a shape this module doesn't handle.
pub fn apply(text: &str, edit: RepoEdit<'_>) -> Option<String> {
    // Work on whole lines, keeping each line's `\n` so the file can be put
    // back together with `concat`.
    let mut owned = text.to_string();
    if !owned.is_empty() && !owned.ends_with('\n') {
        owned.push('\n');
    }
    // RUST: `split_inclusive` keeps the separator on each piece, unlike
    // `lines()`, which drops it. Collecting into `Vec<String>` gives owned
    // lines that can be inserted and removed.
    let mut lines: Vec<String> = owned.split_inclusive('\n').map(String::from).collect();

    match (find_block(&lines)?, edit) {
        (Block::Missing, RepoEdit::Append(entries)) => {
            lines.push("repos:\n".to_string());
            lines.extend(render(entries, 2)?);
        }
        (Block::EmptyFlow { key }, RepoEdit::Append(entries)) => {
            lines[key] = lines[key].replacen(" []", "", 1);
            // RUST: `splice` replaces a range of a Vec with the items of an
            // iterator; an empty range (`key + 1..key + 1`) just inserts.
            lines.splice(key + 1..key + 1, render(entries, 2)?);
        }
        (Block::Seq { key, indent, items }, RepoEdit::Append(entries)) => {
            let at = items.last().map_or(key + 1, |&(_, end)| end);
            lines.splice(at..at, render(entries, indent)?);
        }
        (Block::Seq { key, items, .. }, RepoEdit::Remove(index)) => {
            let &(start, end) = items.get(index)?;
            lines.drain(start..end);
            if items.len() == 1 {
                // Removing the only entry: `repos:` with nothing under it
                // would parse as null, so write an explicit empty list.
                lines[key] = lines[key].replacen("repos:", "repos: []", 1);
            }
        }
        // Nothing to remove from an empty or missing list.
        (Block::Missing | Block::EmptyFlow { .. }, RepoEdit::Remove(_)) => return None,
    }
    Some(lines.concat())
}

/// Serialize `entries` as YAML list items indented by `indent` spaces.
fn render(entries: &[RepoEntry], indent: usize) -> Option<Vec<String>> {
    // serde_yaml_ng writes a top-level sequence as `- a\n- url: b\n  path: c\n`,
    // so indenting every line keeps nested keys lined up under their item.
    let yaml = serde_yaml_ng::to_string(entries).ok()?;
    let pad = " ".repeat(indent);
    Some(
        yaml.split_inclusive('\n')
            .map(|line| format!("{pad}{line}"))
            .collect(),
    )
}

/// Leading spaces on `line`. Tabs aren't allowed for indentation in YAML.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A line that carries no YAML content: blank, or only a comment.
fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `- item` or a bare `-` after the indentation.
fn is_item(line: &str, indent: usize) -> bool {
    let rest = line[indent..].trim_end();
    rest == "-" || rest.starts_with("- ")
}

/// What follows `repos:` on its line, with any trailing comment removed.
fn value_after_key(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("repos:")?;
    // `repos:x` is not the `repos` key.
    if !(rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n'])) {
        return None;
    }
    // RUST: `split_once` returns `Option<(&str, &str)>`; `map_or` gives a
    // default when the separator isn't there.
    let value = rest.split_once(" #").map_or(rest, |(v, _)| v);
    Some(value.trim())
}

fn find_block(lines: &[String]) -> Option<Block> {
    // RUST: `position` returns the index of the first match, or `None`.
    let Some(key) = lines.iter().position(|l| value_after_key(l).is_some()) else {
        return Some(Block::Missing);
    };
    match value_after_key(&lines[key])? {
        "" => {}
        "[]" => return Some(Block::EmptyFlow { key }),
        // A flow list with entries, an anchor, a scalar...: not handled.
        _ => return None,
    }

    let mut indent: Option<usize> = None;
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(key + 1) {
        let col = indent_of(line);
        if is_blank_or_comment(line) {
            // A comment indented deeper than the items belongs to the item
            // above it (so removing the item removes it too). Other comments
            // and blank lines are left where they are.
            if let (Some(first), Some((_, end))) = (indent, items.last_mut())
                && !line.trim().is_empty()
                && col > first
            {
                *end = i + 1;
            }
            continue;
        }
        if col == 0 && !is_item(line, 0) {
            // The next top-level key (or `---`): the list is over.
            break;
        }
        let first = *indent.get_or_insert(col);
        if col == first && is_item(line, col) {
            items.push((i, i + 1));
        } else if col > first {
            // A continuation of the current item: extend it to this line.
            // RUST: `last_mut` gives a mutable reference into the Vec, so
            // the tuple can be updated in place.
            let (_, end) = items.last_mut()?;
            *end = i + 1;
        } else {
            // Shallower than the items, or a non-item at item depth.
            return None;
        }
    }
    Some(Block::Seq {
        key,
        indent: indent.unwrap_or(2),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(s: &str) -> RepoEntry {
        RepoEntry::Short(s.to_string())
    }

    const COMMENTED: &str = "\
# my repos
root: ~/src   # where they live

repos:
  # work
  - work/api   # the API
  - url: work/web
    path: web   # custom path
    # trailing note inside the item

  # personal
  - me/dotfiles
# after the list
other: 1
";

    #[test]
    fn append_after_last_item_keeps_comments() {
        let out = apply(COMMENTED, RepoEdit::Append(&[short("me/new")])).unwrap();
        assert!(
            out.contains("  - me/dotfiles\n  - me/new\n# after the list\n"),
            "{out}"
        );
        assert!(out.starts_with("# my repos\nroot: ~/src   # where they live\n"));
    }

    #[test]
    fn append_full_entry_is_indented_as_a_block() {
        let entry = RepoEntry::Full {
            url: "o/n".into(),
            path: Some("x".into()),
            branch: None,
            identity: None,
        };
        let out = apply(
            "root: /r\nrepos:\n- a/b\n",
            RepoEdit::Append(std::slice::from_ref(&entry)),
        )
        .unwrap();
        assert_eq!(out, "root: /r\nrepos:\n- a/b\n- url: o/n\n  path: x\n");
    }

    #[test]
    fn append_to_empty_or_missing_list() {
        let new = [short("a/b")];
        assert_eq!(
            apply("root: /r\nrepos: []  # none yet\n", RepoEdit::Append(&new)).unwrap(),
            "root: /r\nrepos:  # none yet\n  - a/b\n"
        );
        assert_eq!(
            apply("root: /r", RepoEdit::Append(&new)).unwrap(),
            "root: /r\nrepos:\n  - a/b\n"
        );
    }

    #[test]
    fn remove_middle_item_with_its_nested_lines() {
        let out = apply(COMMENTED, RepoEdit::Remove(1)).unwrap();
        assert!(
            !out.contains("work/web") && !out.contains("custom path"),
            "{out}"
        );
        assert!(!out.contains("trailing note"), "{out}");
        assert!(out.contains("  - work/api   # the API\n\n  # personal\n  - me/dotfiles\n"));
    }

    #[test]
    fn remove_only_item_leaves_empty_list() {
        assert_eq!(
            apply("root: /r\nrepos:\n  - a/b\nx: 1\n", RepoEdit::Remove(0)).unwrap(),
            "root: /r\nrepos: []\nx: 1\n"
        );
    }

    #[test]
    fn unsupported_shapes_are_refused() {
        let new = [short("a/b")];
        assert!(apply("root: /r\nrepos: [x/y]\n", RepoEdit::Append(&new)).is_none());
        assert!(apply("root: /r\nrepos:\n  - a\n - b\n", RepoEdit::Append(&new)).is_none());
        assert!(apply("root: /r\nrepos:\n  - a\n", RepoEdit::Remove(3)).is_none());
        assert!(apply("root: /r\n", RepoEdit::Remove(0)).is_none());
    }
}
