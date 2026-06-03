//! `r{char}` replace operator.
//!
//! Normal mode: `r<c>` replaces the character under the cursor with `<c>`;
//! cursor stays put. With a count, replaces `count` characters on the
//! current line (capped at end-of-line).
//!
//! Visual / visual-line modes: replaces every character in the selection
//! with `<c>`, preserving newlines.
//!
//! Visual-block: replaces every cell of the rectangle with `<c>`.
//!
//! The `r` keypress fires [`enter_replace`] which only sets a pending flag
//! on the editor; the mode handlers consume the next typed key as the
//! replacement char. This keeps the keymap trie free of "any-char" wildcards.

use std::sync::Arc;

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::keymap::{Action, ActionRegistry, KeymapRegistry, Key, KeyCode};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("enter_replace", Arc::new(enter_replace));
    reg.register("toggle_case", Arc::new(toggle_case_at_cursor));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    let modes = [
        ModeId::Normal,
        ModeId::Visual,
        ModeId::VisualLine,
        ModeId::VisualBlock,
    ];
    for m in modes {
        reg.bind(m, "r", Action::Builtin("enter_replace")).unwrap();
    }
    // `~` toggles the case of the character(s) under the cursor and advances.
    reg.bind(ModeId::Normal, "~", Action::Builtin("toggle_case")).unwrap();
}

fn enter_replace(editor: &mut Editor) {
    editor.pending_replace = true;
}

/// Consume `key` as the replacement char if one is pending. Returns true if
/// the key was absorbed. Called from each mode's `handle_key`.
pub fn try_consume_replacement(editor: &mut Editor, key: Key) -> bool {
    if !editor.pending_replace {
        return false;
    }
    editor.pending_replace = false;
    // Esc cancels.
    if matches!(key.code, KeyCode::Esc) {
        editor.clear_pending_count();
        return true;
    }
    let replacement = match key.code {
        KeyCode::Char(c) => c,
        KeyCode::Enter => '\n',
        KeyCode::Tab => '\t',
        _ => {
            editor.clear_pending_count();
            return true;
        }
    };
    match editor.mode {
        ModeId::Normal => replace_at_cursor(editor, replacement),
        ModeId::Visual | ModeId::VisualLine => {
            replace_charwise_selection(editor, replacement);
            return_to_normal(editor);
        }
        ModeId::VisualBlock => {
            replace_block(editor, replacement);
            return_to_normal(editor);
        }
        _ => {}
    }
    true
}

fn return_to_normal(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    crate::mode::switch_mode(editor, ModeId::Normal);
}

// ---- Normal-mode replace ---------------------------------------------------

fn cursor_to_char(buf: &Buffer, c: Cursor, tw: usize) -> usize {
    let line_start = buf.line_to_char(c.row);
    let line = buf.line_string(c.row);
    let byte = twidth::col_to_byte(&line, c.col, tw);
    line_start + line[..byte].chars().count()
}

fn replace_at_cursor(editor: &mut Editor, c: char) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let cursor = editor.windows.get(&win_id).unwrap().cursor;
    let tw = editor.config.options.tab_width;

    let (lo, hi, replacement) = {
        let b = editor.buffers.get(&buf_id).unwrap();
        let line = b.line_string(cursor.row);
        let line_start = b.line_to_char(cursor.row);
        let line_chars = line.chars().count();
        let cursor_byte = twidth::col_to_byte(&line, cursor.col, tw);
        let cursor_char_off = line[..cursor_byte].chars().count();
        // Vim refuses to replace if count exceeds remaining chars on line.
        // For v1, cap silently — feels less surprising than no-op.
        let remaining = line_chars.saturating_sub(cursor_char_off);
        let n = count.min(remaining);
        if n == 0 {
            return;
        }
        let lo = line_start + cursor_char_off;
        let hi = lo + n;
        let replacement: String = std::iter::repeat(c).take(n).collect();
        (lo, hi, replacement)
    };
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        let edit = b.replace(lo..hi, &replacement);
        crate::event::emit(
            editor,
            crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
        );
    }
    // Vim leaves the cursor on the *last* replaced cell (count > 1) or on
    // the same cell (count == 1, since the cell itself was replaced).
    if count > 1 {
        if let Some(w) = editor.windows.get_mut(&win_id) {
            // Move cursor right by (count-1). The replacement is the same
            // width as what it replaced for ASCII; for safety we re-derive.
            let b = editor.buffers.get(&buf_id).unwrap();
            let line = b.line_string(cursor.row);
            let cap = twidth::line_display_width(&line, tw).saturating_sub(1);
            w.cursor.col = (cursor.col + count - 1).min(cap);
            w.cursor.sticky_col = w.cursor.col;
        }
    }
}

// ---- Case toggle (`~`) -----------------------------------------------------

/// Swap the case of each character: upper→lower, lower→upper, others
/// unchanged. Handles Unicode expansions (e.g. `ß`→`SS`) by appending the
/// full case mapping rather than assuming a 1:1 char swap.
fn toggle_case_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_uppercase() {
            out.extend(c.to_lowercase());
        } else if c.is_lowercase() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `~` — toggle the case of `count` characters starting at the cursor
/// (capped at end-of-line, never crossing into the next line), then move
/// the cursor just past the last changed character. So `3~` on `test`
/// yields `TESt` with the cursor on the final `t`.
fn toggle_case_at_cursor(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let cursor = editor.windows.get(&win_id).unwrap().cursor;
    let tw = editor.config.options.tab_width;

    let (lo, hi, replacement, new_char_off) = {
        let b = editor.buffers.get(&buf_id).unwrap();
        let line = b.line_string(cursor.row);
        let line_start = b.line_to_char(cursor.row);
        let line_chars = line.chars().count();
        let cursor_byte = twidth::col_to_byte(&line, cursor.col, tw);
        let cursor_char_off = line[..cursor_byte].chars().count();
        let remaining = line_chars.saturating_sub(cursor_char_off);
        let n = count.min(remaining);
        if n == 0 {
            return; // empty line / cursor past content — nothing to toggle
        }
        let source: String = line.chars().skip(cursor_char_off).take(n).collect();
        let replacement = toggle_case_str(&source);
        let lo = line_start + cursor_char_off;
        // Cursor advances by `n`, but never past the last character of the line.
        let new_char_off = if cursor_char_off + n >= line_chars {
            line_chars.saturating_sub(1)
        } else {
            cursor_char_off + n
        };
        (lo, lo + n, replacement, new_char_off)
    };
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        let edit = b.replace(lo..hi, &replacement);
        crate::event::emit(
            editor,
            crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
        );
    }
    // Re-derive the display column for `new_char_off` on the edited line.
    if let Some(b) = editor.buffers.get(&buf_id) {
        let line = b.line_string(cursor.row);
        let mut byte = line.len();
        for (i, (b_off, _)) in line.char_indices().enumerate() {
            if i == new_char_off {
                byte = b_off;
                break;
            }
        }
        let col = twidth::byte_to_col(&line, byte, tw);
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor.col = col;
            w.cursor.sticky_col = col;
        }
    }
}

// ---- Visual character-wise / line-wise replace -----------------------------

fn replace_charwise_selection(editor: &mut Editor, c: char) {
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let tw = editor.config.options.tab_width;
    let cursor = win.cursor;
    let (lo, hi) = match win.selection {
        Selection::Char { anchor } => {
            let (a, b) = if (anchor.row, anchor.col) <= (cursor.row, cursor.col) {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            };
            let buf = editor.buffers.get(&buf_id).unwrap();
            let s = cursor_to_char(buf, a, tw);
            let e = cursor_to_char(buf, b, tw);
            // Inclusive end: bump one char past `e`.
            let total = buf.len_chars();
            (s, (e + 1).min(total))
        }
        Selection::Line { anchor_row } => {
            let (top, bot) = (anchor_row.min(cursor.row), anchor_row.max(cursor.row));
            let buf = editor.buffers.get(&buf_id).unwrap();
            let s = buf.line_to_char(top);
            let last_row = buf.line_count().saturating_sub(1);
            let e = if bot + 1 > last_row {
                buf.len_chars()
            } else {
                buf.line_to_char(bot + 1)
            };
            (s, e)
        }
        _ => return,
    };
    if hi <= lo {
        return;
    }
    let source: String = editor
        .buffers
        .get(&buf_id)
        .unwrap()
        .rope()
        .slice(lo..hi)
        .to_string();
    let replacement: String = source
        .chars()
        .map(|orig| if orig == '\n' { '\n' } else { c })
        .collect();
    // Wrap in a transaction so undo restores the whole selection at once.
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        b.begin_transaction();
        let edit = b.replace(lo..hi, &replacement);
        b.end_transaction();
        crate::event::emit(
            editor,
            crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
        );
    }
    // Land cursor at the start of the selection (vim's behaviour for `r` in visual).
    if let Some(b) = editor.buffers.get(&buf_id) {
        let row = b.char_to_line(lo);
        let line_start = b.line_to_char(row);
        let off_chars = lo.saturating_sub(line_start);
        let line = b.line_string(row);
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_chars {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, tw);
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = row;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
        }
    }
}

// ---- Visual-block replace --------------------------------------------------

fn replace_block(editor: &mut Editor, c: char) {
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let anchor = match win.selection {
        Selection::Block { anchor } => anchor,
        _ => return,
    };
    let tw = editor.config.options.tab_width;
    let (top, bot) = (anchor.row.min(cursor.row), anchor.row.max(cursor.row));
    let (left, right) = (anchor.col.min(cursor.col), anchor.col.max(cursor.col));

    // Compute per-row (lo_char, hi_char, n_chars) ranges first.
    let mut row_ranges: Vec<(usize, usize, usize)> = Vec::new();
    {
        let b = match editor.buffers.get(&buf_id) {
            Some(b) => b,
            None => return,
        };
        for row in top..=bot {
            if row >= b.line_count() {
                break;
            }
            let line = b.line_string(row);
            let left_byte = twidth::col_to_byte(&line, left, tw);
            let right_byte = twidth::col_to_byte(&line, right + 1, tw);
            if right_byte <= left_byte {
                continue;
            }
            let line_start = b.line_to_char(row);
            let left_chars = line[..left_byte].chars().count();
            let inner_chars = line[left_byte..right_byte].chars().count();
            row_ranges.push((
                line_start + left_chars,
                line_start + left_chars + inner_chars,
                inner_chars,
            ));
        }
    }

    if row_ranges.is_empty() {
        return;
    }
    // Apply replacements bottom-up inside a single transaction.
    let mut last_edit = None;
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        b.begin_transaction();
        for (lo, hi, n) in row_ranges.iter().rev() {
            let replacement: String = std::iter::repeat(c).take(*n).collect();
            last_edit = Some(b.replace(*lo..*hi, &replacement));
        }
        b.end_transaction();
    }
    if let Some(edit) = last_edit {
        crate::event::emit(
            editor,
            crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
        );
    }
    // Park the cursor at the top-left of the rectangle.
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = top;
        w.cursor.col = left;
        w.cursor.sticky_col = left;
    }
}
