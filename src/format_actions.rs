//! `gq` formatting operator.
//!
//! Operates on a **line range** (visual selection, `gqq` for the current
//! line + count, or a line motion). Never the whole file.
//!
//! Dispatch per range, mirroring how vim/neovim drive `gq`:
//!  - **Comment lines** → reflow (line-wrap) to `options.textwidth`, keeping
//!    the indent + comment-leader prefix. This is the built-in path, same as
//!    vim's internal `gq` on a comment paragraph.
//!  - **Code** → ask the LSP for `textDocument/rangeFormatting` (like
//!    neovim's `formatexpr=v:lua.vim.lsp.formatexpr()`). If no server offers
//!    range formatting, fall back to an external formatter configured in
//!    `[formatters]` (vim's `formatprg`), piping the selection through it.
//!    The external fallback is **off by default** (empty `[formatters]`).

use std::sync::Arc;

use crate::cursor::Selection;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("format_line", Arc::new(format_line));
    reg.register("visual_format", Arc::new(visual_format));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    // `gqq` / `gqgq` — current line (with count). A few line motions too.
    let normal = [
        ("gqq", "format_line"),
        ("gqgq", "format_line"),
    ];
    for (seq, action) in normal {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
    // `gq` in any visual mode formats the selection.
    for mode in [ModeId::Visual, ModeId::VisualLine, ModeId::VisualBlock] {
        reg.bind(mode, "gq", Action::Builtin("visual_format")).unwrap();
    }
}

// ---- entry points ----------------------------------------------------------

/// `gqq` — format `count` lines starting at the cursor row.
fn format_line(editor: &mut Editor) {
    let count = editor.take_count().max(1);
    let Some(win) = editor.active_window() else { return };
    let top = win.cursor.row;
    let Some(buf_id) = editor.active_buffer_id() else { return };
    let last = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0);
    let bot = (top + count - 1).min(last);
    format_range(editor, top, bot);
    place_cursor_line_start(editor, top);
}

/// `gq` in visual mode — format the selected lines.
fn visual_format(editor: &mut Editor) {
    let Some((top, bot)) = selection_row_range(editor) else {
        switch_mode(editor, ModeId::Normal);
        return;
    };
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    format_range(editor, top, bot);
    place_cursor_line_start(editor, top);
    switch_mode(editor, ModeId::Normal);
}

// ---- core dispatch ---------------------------------------------------------

fn format_range(editor: &mut Editor, lo: usize, hi: usize) {
    let Some(buf_id) = editor.active_buffer_id() else { return };
    let filetype = editor.syntax_for(buf_id).filetype.to_string();
    let tab_width = editor.config.options.tab_width;

    let lines: Vec<String> = {
        let Some(buf) = editor.buffers.get(&buf_id) else { return };
        let last = buf.line_count().saturating_sub(1);
        let hi = hi.min(last);
        (lo..=hi).map(|r| buf.line_string(r)).collect()
    };
    if lines.is_empty() {
        return;
    }
    let hi = lo + lines.len() - 1;

    // Comments → reflow to textwidth.
    if let Some(marker) = line_comment_marker(&filetype) {
        if is_comment_block(&lines, marker) {
            let textwidth = editor.config.options.textwidth.max(1);
            let wrapped = reflow_comment(&lines, marker, textwidth, tab_width);
            replace_lines(editor, buf_id, lo, hi, &wrapped);
            return;
        }
    }

    // Code → LSP range formatting first.
    if format_via_lsp(editor, buf_id, lo as u32, hi as u32) {
        return;
    }

    // Code → external formatter fallback (off unless configured).
    if format_via_external(editor, buf_id, lo, hi, &filetype) {
        return;
    }

    editor.status_message = Some(format!("gq: no formatter for {filetype}"));
}

// ---- comment reflow --------------------------------------------------------

/// Line-comment leader for a filetype, or `None` if we don't reflow comments
/// for it. Only line comments (not block comments) are handled.
fn line_comment_marker(filetype: &str) -> Option<&'static str> {
    Some(match filetype {
        "rust" | "c" | "cpp" | "javascript" | "typescript" | "go" | "java" => "//",
        "python" | "sh" | "ruby" | "toml" | "yaml" | "make" | "dockerfile"
        | "gitconfig" | "perl" | "r" => "#",
        "lua" | "sql" | "haskell" => "--",
        "vim" => "\"",
        "tex" => "%",
        _ => return None,
    })
}

/// True when every non-blank line in `lines`, after its indent, begins with
/// `marker` — i.e. the whole range is a line-comment block.
fn is_comment_block(lines: &[String], marker: &str) -> bool {
    let mut saw_comment = false;
    for l in lines {
        let t = l.trim_start();
        if t.is_empty() {
            continue; // blank lines are allowed inside the block
        }
        if !t.starts_with(marker) {
            return false;
        }
        saw_comment = true;
    }
    saw_comment
}

/// Reflow a comment block to `textwidth` display columns, preserving the
/// leading indent + comment marker. Blank comment lines split paragraphs.
fn reflow_comment(
    lines: &[String],
    marker: &str,
    textwidth: usize,
    tab_width: usize,
) -> String {
    // Derive the prefix from the first non-blank comment line: indent + marker
    // + the run of spaces that follows it (e.g. "    // ").
    let first = lines
        .iter()
        .find(|l| !l.trim_start().is_empty())
        .cloned()
        .unwrap_or_default();
    let indent: String = first.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let after_indent = &first[indent.len()..];
    let after_marker = after_indent.strip_prefix(marker).unwrap_or(after_indent);
    let marker_spaces: String = after_marker.chars().take_while(|c| *c == ' ').collect();
    let space: &str = if marker_spaces.is_empty() { " " } else { marker_spaces.as_str() };
    let prefix = format!("{indent}{marker}{space}");
    let prefix_width = twidth::line_display_width(&prefix, tab_width);
    // At least room for one word per line.
    let target = textwidth.max(prefix_width + 1);

    // Strip the comment leader from each line to get its text content.
    let strip = |l: &str| -> String {
        let t = l.trim_start();
        let rest = t.strip_prefix(marker).unwrap_or(t);
        rest.trim().to_string()
    };

    // Group into paragraphs separated by blank comment lines, reflow each.
    let mut out_lines: Vec<String> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let flush = |para: &mut Vec<String>, out: &mut Vec<String>| {
        if para.is_empty() {
            return;
        }
        let words: Vec<String> = para
            .join(" ")
            .split_whitespace()
            .map(|w| w.to_string())
            .collect();
        para.clear();
        if words.is_empty() {
            return;
        }
        let mut cur = String::new();
        let mut cur_w = prefix_width;
        for w in words {
            let ww = twidth::line_display_width(&w, tab_width);
            if cur.is_empty() {
                cur = w;
                cur_w = prefix_width + ww;
            } else if cur_w + 1 + ww <= target {
                cur_w += 1 + ww;
                cur.push(' ');
                cur.push_str(&w);
            } else {
                out.push(std::mem::take(&mut cur));
                cur = w;
                cur_w = prefix_width + ww;
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    };

    for l in lines {
        if strip(l).is_empty() {
            // Paragraph break: flush the current paragraph, keep one blank
            // comment line as the separator.
            flush(&mut para, &mut out_lines);
            out_lines.push(String::new());
        } else {
            para.push(strip(l));
        }
    }
    flush(&mut para, &mut out_lines);

    // Re-attach the prefix. Empty entries become a bare "indent + marker" line.
    let bare = format!("{indent}{marker}");
    let rendered: Vec<String> = out_lines
        .iter()
        .map(|l| if l.is_empty() { bare.clone() } else { format!("{prefix}{l}") })
        .collect();
    // Drop a trailing blank-comment line introduced by a final paragraph break.
    let mut rendered = rendered;
    while rendered.last().map(|s| s == &bare).unwrap_or(false) {
        rendered.pop();
    }
    rendered.join("\n")
}

// ---- LSP range formatting --------------------------------------------------

fn format_via_lsp(editor: &mut Editor, buf_id: crate::buffer::BufferId, lo: u32, hi: u32) -> bool {
    let path = match editor.buffers.get(&buf_id).and_then(|b| b.path()) {
        Some(p) => p.to_path_buf(),
        None => return false,
    };
    let Ok(uri) = lsp_types::Url::from_file_path(&path) else {
        return false;
    };
    let filetype = editor.syntax_for(buf_id).filetype.to_string();
    let tab_size = editor.config.options.tab_width as u32;
    let insert_spaces = editor.config.options.expandtab;

    let edits = {
        let Some(client) = editor
            .lsp
            .client_for(&filetype, &path, |c| c.range_formatting)
        else {
            return false;
        };
        client.range_formatting(uri.as_str(), lo, hi, tab_size, insert_spaces)
    };
    match edits {
        Some(edits) if !edits.is_empty() => {
            crate::lsp_apply::apply_text_edits_to_buffer(editor, buf_id, &edits);
            true
        }
        // The server answered (range formatting supported) but had no changes.
        Some(_) => true,
        None => false,
    }
}

// ---- external formatter -----------------------------------------------------

fn format_via_external(
    editor: &mut Editor,
    buf_id: crate::buffer::BufferId,
    lo: usize,
    hi: usize,
    filetype: &str,
) -> bool {
    let Some(cmd) = editor.config.formatters.get(filetype).cloned() else {
        return false;
    };
    if cmd.is_empty() {
        return false;
    }
    let input: String = {
        let Some(buf) = editor.buffers.get(&buf_id) else { return false };
        (lo..=hi).map(|r| buf.line_string(r) + "\n").collect()
    };
    match run_formatter(&cmd, &input) {
        Ok(out) if !out.is_empty() => {
            let out = out.strip_suffix('\n').unwrap_or(&out).to_string();
            replace_lines(editor, buf_id, lo, hi, &out);
            true
        }
        Ok(_) => {
            editor.status_message = Some(format!("gq: {} produced no output", cmd[0]));
            true
        }
        Err(e) => {
            editor.status_message = Some(format!("gq: {} failed: {e}", cmd[0]));
            true
        }
    }
}

/// Run `argv` with `input` on stdin, returning stdout. Errors on non-zero exit.
fn run_formatter(argv: &[String], input: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("non-zero exit").to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---- shared helpers --------------------------------------------------------

/// Replace lines `[lo, hi]` (inclusive) with `new_text` as one undo step,
/// emitting a single `BufferChanged`.
fn replace_lines(
    editor: &mut Editor,
    buf_id: crate::buffer::BufferId,
    lo: usize,
    hi: usize,
    new_text: &str,
) {
    let (lo_char, hi_char, had_trailing_nl) = {
        let Some(buf) = editor.buffers.get(&buf_id) else { return };
        let last = buf.line_count().saturating_sub(1);
        let hi = hi.min(last);
        let lo_char = buf.line_to_char(lo);
        let (hi_char, had_nl) = if hi >= last {
            (buf.len_chars(), buf.rope().to_string().ends_with('\n'))
        } else {
            (buf.line_to_char(hi + 1), true)
        };
        (lo_char, hi_char, had_nl)
    };
    // Keep the block a set of complete lines: end with a newline unless we
    // replaced the final line of a file that had no trailing newline.
    let mut replacement = new_text.to_string();
    if had_trailing_nl && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    let edit = {
        let Some(buf) = editor.buffers.get_mut(&buf_id) else { return };
        buf.replace(lo_char..hi_char, &replacement)
    };
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );
}

fn place_cursor_line_start(editor: &mut Editor, row: usize) {
    let Some(buf_id) = editor.active_buffer_id() else { return };
    let last = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0);
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = row.min(last);
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
}

fn selection_row_range(editor: &Editor) -> Option<(usize, usize)> {
    let win = editor.active_window()?;
    let lo_hi = |a: usize, b: usize| if a <= b { (a, b) } else { (b, a) };
    Some(match win.selection {
        Selection::None => return None,
        Selection::Char { anchor } => lo_hi(anchor.row, win.cursor.row),
        Selection::Line { anchor_row } => lo_hi(anchor_row, win.cursor.row),
        Selection::Block { anchor } => lo_hi(anchor.row, win.cursor.row),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_comment_block() {
        let lines = vec!["// foo".to_string(), "// bar".to_string()];
        assert!(is_comment_block(&lines, "//"));
        let mixed = vec!["// foo".to_string(), "let x = 1;".to_string()];
        assert!(!is_comment_block(&mixed, "//"));
    }

    #[test]
    fn reflow_wraps_to_textwidth() {
        let lines = vec![
            "// the quick brown fox jumps over the lazy dog".to_string(),
        ];
        // width 20 → prefix "// " is 3 cols, so ~17 cols of text per line.
        let out = reflow_comment(&lines, "//", 20, 4);
        for l in out.lines() {
            assert!(l.starts_with("// "));
            assert!(twidth::line_display_width(l, 4) <= 20, "line too wide: {l:?}");
        }
        // No words lost.
        let joined: String = out.lines().map(|l| l.trim_start_matches("// ")).collect::<Vec<_>>().join(" ");
        assert_eq!(joined, "the quick brown fox jumps over the lazy dog");
    }

    #[test]
    fn reflow_preserves_indent_and_joins_short_lines() {
        let lines = vec![
            "    # alpha".to_string(),
            "    # beta".to_string(),
            "    # gamma".to_string(),
        ];
        let out = reflow_comment(&lines, "#", 80, 4);
        assert_eq!(out, "    # alpha beta gamma");
    }
}
