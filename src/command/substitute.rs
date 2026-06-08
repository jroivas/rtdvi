//! `:[range]s/pattern/replacement/[flags]` — regex search-and-replace.
//!
//! Ranges: `%` (whole buffer), `'<,'>` (last visual selection), absolute
//! line numbers, `.` (current line), `$` (last line), and `N,M` pairs. No
//! range means the current line.
//!
//! Patterns use the same Rust [`regex`] syntax as `/` search. The
//! replacement understands vim-style `\1`..`\9` capture references and `&`
//! for the whole match; a literal `$` is emitted verbatim. Flags: `g`
//! (every match per line, not just the first) and `i` (case-insensitive).
//!
//! [`try_run`] is the entry point, called from `run_ex_line` before the
//! generic command parser (which has no concept of ranges).

use regex::Regex;

use crate::Editor;

/// Attempt to interpret `norm` (the ex line, leading `:` already stripped) as
/// a substitution. Returns `true` when it was one — handled, including the
/// error path — so the caller stops. Returns `false` to fall through to the
/// normal command parser.
pub fn try_run(editor: &mut Editor, norm: &str) -> bool {
    let (range, rest) = parse_range(editor, norm);
    let rest = rest.trim_start();

    // Command head: `s` or `substitute`, immediately followed by a delimiter
    // (a non-alphanumeric, non-space char). The delimiter check is what keeps
    // `:set`, `:sort`, `:sp`, … from being mistaken for a substitution.
    let body = if let Some(b) = rest.strip_prefix("substitute") {
        b
    } else if let Some(b) = rest.strip_prefix('s') {
        b
    } else {
        return false;
    };
    let delim = match body.chars().next() {
        Some(c) if !c.is_alphanumeric() && !c.is_whitespace() => c,
        _ => return false,
    };

    let (first, last) = range.unwrap_or_else(|| {
        let r = editor.active_window().map(|w| w.cursor.row).unwrap_or(0);
        (r, r)
    });
    run(editor, first, last, delim, &body[delim.len_utf8()..]);
    true
}

/// Run the substitution over inclusive line range `first..=last`. `content`
/// is everything after the opening delimiter (`pattern<delim>replacement<delim>flags`).
fn run(editor: &mut Editor, first: usize, last: usize, delim: char, content: &str) {
    let segs = split_on_delim(content, delim);
    let mut pattern = segs.first().cloned().unwrap_or_default();
    let replacement = translate_replacement(&segs.get(1).cloned().unwrap_or_default());
    let flags = segs.get(2).cloned().unwrap_or_default();

    // Empty pattern reuses the last search pattern, like vim's `:%s//repl/`.
    if pattern.is_empty() {
        match editor.search.last_pattern.clone() {
            Some(p) => pattern = p,
            None => {
                editor.status_message = Some("E35: no previous regular expression".into());
                return;
            }
        }
    }

    let global = flags.contains('g');
    let full_pattern = if flags.contains('i') {
        format!("(?i){pattern}")
    } else {
        pattern.clone()
    };
    let re = match Regex::new(&full_pattern) {
        Ok(re) => re,
        Err(e) => {
            editor.status_message = Some(format!("E: invalid pattern: {e}"));
            return;
        }
    };

    let Some(buf_id) = editor.active_buffer_id() else { return };
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        b.materialize(); // build the rope for large files before editing
    }
    let Some(buf) = editor.buffers.get(&buf_id) else { return };

    let lc = buf.line_count();
    if lc == 0 {
        return;
    }
    let first = first.min(lc - 1);
    let last = last.min(lc - 1);

    let start_char = buf.line_to_char(first);
    let end_char = if last + 1 >= lc {
        buf.len_chars()
    } else {
        buf.line_to_char(last + 1)
    };
    let had_trailing_nl =
        end_char > start_char && buf.rope().slice(end_char - 1..end_char).to_string() == "\n";

    let mut out = String::new();
    let mut total = 0usize;
    let mut changed_lines = 0usize;
    let mut last_changed = first;
    for row in first..=last {
        let line = buf.line_string(row);
        let n = re.find_iter(&line).count();
        if n == 0 {
            out.push_str(&line);
        } else {
            let new_line = if global {
                re.replace_all(&line, replacement.as_str()).into_owned()
            } else {
                re.replacen(&line, 1, replacement.as_str()).into_owned()
            };
            total += if global { n } else { 1 };
            changed_lines += 1;
            last_changed = row;
            out.push_str(&new_line);
        }
        if row < last {
            out.push('\n');
        }
    }
    if had_trailing_nl {
        out.push('\n');
    }

    if total == 0 {
        editor.status_message = Some(format!("E486: Pattern not found: {pattern}"));
        return;
    }

    let edit = {
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        buf.replace(start_char..end_char, &out)
    };
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );

    // Park the cursor on the last line that changed (clamped to the buffer,
    // which may have shrunk if the replacement removed newlines).
    let new_last = editor
        .buffers
        .get(&buf_id)
        .map_or(0, |b| b.line_count().saturating_sub(1));
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = last_changed.min(new_last);
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
        w.selection = crate::cursor::Selection::None;
    }
    editor.status_message = Some(format!(
        "{total} substitution{} on {changed_lines} line{}",
        if total == 1 { "" } else { "s" },
        if changed_lines == 1 { "" } else { "s" },
    ));
}

/// Consume an optional leading range spec, returning `(range, remaining)`.
/// `range` is an inclusive 0-based `(first, last)`, or `None` when no range
/// was present.
fn parse_range<'a>(editor: &Editor, s: &'a str) -> (Option<(usize, usize)>, &'a str) {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('%') {
        return (Some((0, last_line(editor))), rest);
    }
    let Some((a, rest)) = parse_addr(editor, s) else {
        return (None, s);
    };
    if let Some(rest) = rest.strip_prefix(',') {
        if let Some((b, rest)) = parse_addr(editor, rest) {
            return (Some((a.min(b), a.max(b))), rest);
        }
        return (Some((a, a)), rest);
    }
    (Some((a, a)), rest)
}

/// Parse a single line address (`.`, `$`, `N`, `'<`, `'>`), returning the
/// 0-based row and the unconsumed tail.
fn parse_addr<'a>(editor: &Editor, s: &'a str) -> Option<(usize, &'a str)> {
    let last = last_line(editor);
    if let Some(r) = s.strip_prefix('.') {
        let row = editor.active_window().map(|w| w.cursor.row).unwrap_or(0);
        return Some((row.min(last), r));
    }
    if let Some(r) = s.strip_prefix('$') {
        return Some((last, r));
    }
    if let Some(r) = s.strip_prefix("'<") {
        let row = editor.last_visual_range.map_or(0, |(a, _)| a);
        return Some((row.min(last), r));
    }
    if let Some(r) = s.strip_prefix("'>") {
        let row = editor.last_visual_range.map_or(last, |(_, b)| b);
        return Some((row.min(last), r));
    }
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let n: usize = digits.parse().unwrap_or(1);
        return Some((n.saturating_sub(1).min(last), &s[digits.len()..]));
    }
    None
}

fn last_line(editor: &Editor) -> usize {
    editor
        .active_buffer_id()
        .and_then(|id| editor.buffers.get(&id))
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0)
}

/// Split `s` into segments on unescaped `delim`. A `\<delim>` collapses to a
/// literal delimiter inside the segment; every other backslash escape is left
/// intact so regex metacharacters (`\d`, `\(`, …) survive untouched.
fn split_on_delim(s: &str, delim: char) -> Vec<String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.peek() == Some(&delim) {
                cur.push(delim);
                chars.next();
            } else {
                cur.push('\\');
            }
            continue;
        }
        if c == delim {
            segs.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    segs.push(cur);
    segs
}

/// Translate a vim-style replacement into the [`regex`] crate's syntax:
/// `\1`..`\9` → `${1}`..`${9}`, `&` → the whole match, and a literal `$` is
/// escaped to `$$`.
fn translate_replacement(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(d) if d.is_ascii_digit() => out.push_str(&format!("${{{d}}}")),
                Some('&') => out.push('&'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic() {
        assert_eq!(split_on_delim("a/b/g", '/'), vec!["a", "b", "g"]);
        assert_eq!(split_on_delim("a/b", '/'), vec!["a", "b"]);
        assert_eq!(split_on_delim("a", '/'), vec!["a"]);
    }

    #[test]
    fn split_escaped_delim() {
        assert_eq!(split_on_delim(r"a\/b/c", '/'), vec!["a/b", "c"]);
        // Regex escapes survive.
        assert_eq!(split_on_delim(r"\d+/x", '/'), vec![r"\d+", "x"]);
    }

    #[test]
    fn replacement_groups_and_amp() {
        assert_eq!(translate_replacement(r"\1-\2"), "${1}-${2}");
        assert_eq!(translate_replacement("[&]"), "[${0}]");
        assert_eq!(translate_replacement("$x"), "$$x");
        assert_eq!(translate_replacement(r"\&"), "&");
    }
}
