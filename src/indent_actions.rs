//! `>>` and `<<` operators (and their `>` / `<` visual-mode forms)
//! for indenting / dedenting whole lines by one shiftwidth.
//!
//! Indent unit follows `options.expandtab`: spaces when on,
//! a literal tab when off. Counts work (`3>>` indents three lines).
//! All affected lines are coalesced into a single undo step.

use std::sync::Arc;

use crate::cursor::Selection;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("indent_line", Arc::new(indent_line));
    reg.register("dedent_line", Arc::new(dedent_line));
    reg.register("visual_indent", Arc::new(visual_indent));
    reg.register("visual_dedent", Arc::new(visual_dedent));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    // The sequence parser treats `<` as the opener of `<Name>` tokens,
    // so the literal angle brackets need their named-key escapes
    // (`<Lt>` = `<`, `<Gt>` = `>`).
    reg.bind(ModeId::Normal, "<Gt><Gt>", Action::Builtin("indent_line"))
        .unwrap();
    reg.bind(ModeId::Normal, "<Lt><Lt>", Action::Builtin("dedent_line"))
        .unwrap();
    for mode in [ModeId::Visual, ModeId::VisualLine, ModeId::VisualBlock] {
        reg.bind(mode, "<Gt>", Action::Builtin("visual_indent"))
            .unwrap();
        reg.bind(mode, "<Lt>", Action::Builtin("visual_dedent"))
            .unwrap();
    }
}

// ---- Public entry points (also re-used by visual variants) ---------------

fn indent_line(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(top) = editor.active_window().map(|w| w.cursor.row) else {
        return;
    };
    indent_rows(editor, top, top + count);
    place_cursor_first_nonblank(editor, top);
}

fn dedent_line(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(top) = editor.active_window().map(|w| w.cursor.row) else {
        return;
    };
    dedent_rows(editor, top, top + count);
    place_cursor_first_nonblank(editor, top);
}

fn visual_indent(editor: &mut Editor) {
    let count = editor.take_count();
    let Some((top, bot)) = selection_row_range(editor) else {
        return;
    };
    for _ in 0..count {
        indent_rows(editor, top, bot + 1);
    }
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    place_cursor_first_nonblank(editor, top);
    switch_mode(editor, ModeId::Normal);
}

fn visual_dedent(editor: &mut Editor) {
    let count = editor.take_count();
    let Some((top, bot)) = selection_row_range(editor) else {
        return;
    };
    for _ in 0..count {
        dedent_rows(editor, top, bot + 1);
    }
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    place_cursor_first_nonblank(editor, top);
    switch_mode(editor, ModeId::Normal);
}

// ---- Core helpers --------------------------------------------------------

fn indent_unit(editor: &Editor) -> String {
    let opts = &editor.config.options;
    if opts.expandtab {
        " ".repeat(opts.tab_width.max(1))
    } else {
        "\t".to_string()
    }
}

/// Insert the indent unit at the start of every row in `[top, bot)`.
/// Skips truly empty lines (matches vim's `>>` on a blank line being
/// a no-op). Coalesces into one undo step.
fn indent_rows(editor: &mut Editor, top: usize, bot_exclusive: usize) {
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let unit = indent_unit(editor);
    let last_row = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count())
        .unwrap_or(0);
    let bot = bot_exclusive.min(last_row);
    if top >= bot {
        return;
    }
    let opened_here = !editor
        .buffers
        .get(&buf_id)
        .map(|b| b.in_transaction())
        .unwrap_or(false);
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().begin_transaction();
    }
    {
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        for row in top..bot {
            let line = buf.line_string(row);
            if line.is_empty() || line == "\n" {
                continue;
            }
            let lo = buf.line_to_char(row);
            let _ = buf.insert(lo, &unit);
        }
    }
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().end_transaction();
    }
}

/// Remove up to one shiftwidth's worth of leading whitespace from
/// every row in `[top, bot)`. A leading tab counts as `tab_width`
/// columns instantly — same model `<<` uses in vim.
fn dedent_rows(editor: &mut Editor, top: usize, bot_exclusive: usize) {
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let tw = editor.config.options.tab_width.max(1);
    let last_row = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count())
        .unwrap_or(0);
    let bot = bot_exclusive.min(last_row);
    if top >= bot {
        return;
    }
    let opened_here = !editor
        .buffers
        .get(&buf_id)
        .map(|b| b.in_transaction())
        .unwrap_or(false);
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().begin_transaction();
    }
    {
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        for row in top..bot {
            let line = buf.line_string(row);
            let n = leading_indent_bytes_to_drop(&line, tw);
            if n == 0 {
                continue;
            }
            // Convert byte-count to char-count for the rope API. All
            // leading whitespace we count is ASCII, so byte == char.
            let lo = buf.line_to_char(row);
            let _ = buf.delete(lo..lo + n);
        }
    }
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().end_transaction();
    }
}

/// Walk leading whitespace by display column. Tab counts as a jump to
/// the next tab stop (so a leading tab consumes `tab_width` columns
/// immediately). Stop after `tab_width` columns or at the first
/// non-whitespace char. Returns the number of bytes to drop.
fn leading_indent_bytes_to_drop(line: &str, tab_width: usize) -> usize {
    let bytes = line.as_bytes();
    let mut col = 0usize;
    let mut byte = 0usize;
    while byte < bytes.len() && col < tab_width {
        match bytes[byte] {
            b' ' => {
                col += 1;
                byte += 1;
            }
            b'\t' => {
                let stop = tab_width.max(1);
                col += stop - (col % stop);
                byte += 1;
            }
            _ => break,
        }
    }
    byte
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

/// After indent/dedent, vim parks the cursor on the first non-blank
/// of the (now-changed) first affected line. Compute that.
fn place_cursor_first_nonblank(editor: &mut Editor, row: usize) {
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let col = editor
        .buffers
        .get(&buf_id)
        .map(|b| {
            let line = b.line_string(row);
            let mut col = 0usize;
            for (b_off, c) in line.char_indices() {
                if !c.is_whitespace() || c == '\n' {
                    return col;
                }
                if c == '\t' {
                    let stop = tw.max(1);
                    col += stop - (col % stop);
                } else {
                    col += 1;
                }
                let _ = b_off;
            }
            col
        })
        .unwrap_or(0);
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_walks_to_one_shiftwidth() {
        assert_eq!(leading_indent_bytes_to_drop("\tfoo", 4), 1);
        assert_eq!(leading_indent_bytes_to_drop("    foo", 4), 4);
        assert_eq!(leading_indent_bytes_to_drop("  foo", 4), 2);
        assert_eq!(leading_indent_bytes_to_drop("\t\tfoo", 4), 1);
        assert_eq!(leading_indent_bytes_to_drop("foo", 4), 0);
        assert_eq!(leading_indent_bytes_to_drop(" \tfoo", 4), 2);
    }
}
