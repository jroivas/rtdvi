//! Actions specific to visual modes: enter visual, delete/yank/change.

use std::sync::Arc;

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("enter_visual", Arc::new(enter_visual));
    reg.register("enter_visual_line", Arc::new(enter_visual_line));
    reg.register("visual_delete", Arc::new(visual_delete));
    reg.register("visual_yank", Arc::new(visual_yank));
    reg.register("visual_change", Arc::new(visual_change));
    reg.register("paste_after", Arc::new(paste_after));
}

/// Bind enter-visual keys + paste under Normal; mirror motion + d/y/c under
/// the visual modes themselves.
pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    // Normal -> visual entry + paste.
    let normal_bindings = [
        ("v", "enter_visual"),
        ("V", "enter_visual_line"),
        ("p", "paste_after"),
    ];
    for (seq, action) in normal_bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
    // Motions under visual modes (same names as in normal).
    let motions = [
        ("h", "move_left"),
        ("l", "move_right"),
        ("k", "move_up"),
        ("j", "move_down"),
        ("0", "line_start"),
        ("$", "line_end"),
        ("gg", "first_line"),
        ("G", "last_line"),
        ("w", "word_forward"),
        ("b", "word_backward"),
        ("e", "word_end"),
        ("<Left>", "move_left"),
        ("<Right>", "move_right"),
        ("<Up>", "move_up"),
        ("<Down>", "move_down"),
    ];
    for mode in [ModeId::Visual, ModeId::VisualLine, ModeId::VisualBlock] {
        for (seq, action) in motions {
            reg.bind(mode, seq, Action::Builtin(action)).unwrap();
        }
        // d / y / c.
        reg.bind(mode, "d", Action::Builtin("visual_delete")).unwrap();
        reg.bind(mode, "y", Action::Builtin("visual_yank")).unwrap();
        reg.bind(mode, "c", Action::Builtin("visual_change")).unwrap();
        reg.bind(mode, "x", Action::Builtin("visual_delete")).unwrap();
    }
}

fn enter_visual(editor: &mut Editor) {
    let Some(w) = editor.active_window_mut() else {
        return;
    };
    w.selection = Selection::Char { anchor: w.cursor };
    switch_mode(editor, ModeId::Visual);
}

fn enter_visual_line(editor: &mut Editor) {
    let Some(w) = editor.active_window_mut() else {
        return;
    };
    w.selection = Selection::Line { anchor_row: w.cursor.row };
    switch_mode(editor, ModeId::VisualLine);
}

/// Compute the (start_char, end_char) range from a character-wise selection
/// in the active window, plus a flag indicating linewise selection.
pub(crate) fn selection_char_range(editor: &Editor) -> Option<(usize, usize, bool)> {
    let win = editor.active_window()?;
    let buf = editor.buffers.get(&win.buffer)?;
    let tw = editor.config.options.tab_width;
    match win.selection {
        Selection::None => None,
        Selection::Char { anchor } => Some((
            char_index(buf, normalize_pair(anchor, win.cursor).0, tw),
            // End is *exclusive* — bump one char past the head's grapheme.
            {
                let (_, tail) = normalize_pair(anchor, win.cursor);
                let after = step_one_char(buf, tail, tw);
                char_index(buf, after, tw)
            },
            false,
        )),
        Selection::Line { anchor_row } => {
            let cur_row = win.cursor.row;
            let (top, bot) = if anchor_row <= cur_row {
                (anchor_row, cur_row)
            } else {
                (cur_row, anchor_row)
            };
            let start = buf.line_to_char(top);
            // End = start of (bot + 1), capped to buffer end.
            let end = if bot + 1 >= buf.line_count() {
                buf.len_chars()
            } else {
                buf.line_to_char(bot + 1)
            };
            Some((start, end, true))
        }
        Selection::Block { .. } => None,
    }
}

/// `Cursor` representing the smaller of two cursors in document order.
fn normalize_pair(a: Cursor, b: Cursor) -> (Cursor, Cursor) {
    if (a.row, a.col) <= (b.row, b.col) {
        (a, b)
    } else {
        (b, a)
    }
}

fn char_index(buf: &Buffer, cursor: Cursor, tw: usize) -> usize {
    let line_start = buf.line_to_char(cursor.row);
    let line = buf.line_string(cursor.row);
    let byte = twidth::col_to_byte(&line, cursor.col, tw);
    line_start + line[..byte].chars().count()
}

/// Return a cursor positioned one grapheme past `c` (clamped to end of line).
fn step_one_char(buf: &Buffer, c: Cursor, tw: usize) -> Cursor {
    let line = buf.line_string(c.row);
    let byte = twidth::col_to_byte(&line, c.col, tw);
    if byte >= line.len() {
        return c;
    }
    let next_byte = line[byte..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| byte + i)
        .unwrap_or(line.len());
    let next_col = twidth::byte_to_col(&line, next_byte, tw);
    Cursor { row: c.row, col: next_col, sticky_col: next_col }
}

fn visual_delete(editor: &mut Editor) {
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let removed = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        let edit = b.delete(start..end);
        edit.removed
    };
    editor.unnamed_register = crate::editor::Register {
        text: removed,
        linewise,
    };
    // Move cursor to the start of the deletion.
    if let Some(b) = editor.buffers.get(&buf_id) {
        let row = b.char_to_line(start);
        let line = b.line_string(row);
        let line_start = b.line_to_char(row);
        let off_chars = start.saturating_sub(line_start);
        // Find byte offset for that char offset in `line`.
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_chars {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, editor.config.options.tab_width);
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = row;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
            w.selection = Selection::None;
        }
    }
    switch_mode(editor, ModeId::Normal);
}

fn visual_yank(editor: &mut Editor) {
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let text: String = match editor.buffers.get(&buf_id) {
        Some(b) => b.rope().slice(start..end).to_string(),
        None => return,
    };
    editor.unnamed_register = crate::editor::Register { text, linewise };
    switch_to_normal_clear(editor);
}

fn visual_change(editor: &mut Editor) {
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let removed = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        let edit = b.delete(start..end);
        edit.removed
    };
    editor.unnamed_register = crate::editor::Register {
        text: removed,
        linewise,
    };
    if let Some(b) = editor.buffers.get(&buf_id) {
        let row = b.char_to_line(start);
        let line_start = b.line_to_char(row);
        let off_chars = start.saturating_sub(line_start);
        let line = b.line_string(row);
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_chars {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, editor.config.options.tab_width);
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = row;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
            w.selection = Selection::None;
        }
    }
    switch_mode(editor, ModeId::Insert);
}

fn paste_after(editor: &mut Editor) {
    let reg = editor.unnamed_register.clone();
    if reg.text.is_empty() {
        return;
    }
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tw = editor.config.options.tab_width;
    if reg.linewise {
        // Insert at start of next line.
        let row = cursor.row + 1;
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        let insert_at = if row >= buf.line_count() {
            // Append at end-of-buffer; ensure leading newline.
            let end = buf.len_chars();
            // If buffer doesn't end with newline, add one first.
            let needs_nl = end == 0
                || buf.rope().slice(end.saturating_sub(1)..end).to_string() != "\n";
            if needs_nl {
                buf.insert(end, "\n");
            }
            buf.len_chars()
        } else {
            buf.line_to_char(row)
        };
        // Ensure trailing newline on inserted text so the next line stays intact.
        let mut text = reg.text.clone();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let _ = buf.insert(insert_at, &text);
        // Move cursor to first non-blank of the new line (approximation: col 0).
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor.row = row.min(buf.line_count().saturating_sub(1));
            w.cursor.col = 0;
            w.cursor.sticky_col = 0;
        }
    } else {
        // Insert after the cursor's char.
        let buf = editor.buffers.get(&buf_id).unwrap();
        let after = step_one_char(buf, cursor, tw);
        let char_idx = char_index(buf, after, tw);
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        let _ = buf.insert(char_idx, &reg.text);
        // Place cursor on the last inserted char.
        // (Approximation: bump the cursor by reg.text grapheme width on same line.)
        if let Some(w) = editor.windows.get_mut(&win_id) {
            for ch in reg.text.chars() {
                if ch == '\n' {
                    w.cursor.row += 1;
                    w.cursor.col = 0;
                } else {
                    let g = ch.to_string();
                    let wid = twidth::grapheme_width(&g, w.cursor.col, tw).max(1);
                    w.cursor.col += wid;
                }
            }
            // Step back one to land on the last inserted cell.
            if w.cursor.col > 0 {
                w.cursor.col -= 1;
            }
            w.cursor.sticky_col = w.cursor.col;
        }
    }
}

fn switch_to_normal_clear(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    switch_mode(editor, ModeId::Normal);
}
