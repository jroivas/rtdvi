//! `d{motion}` operator family, plus line-based variants (`dd`, `dj`, `dk`,
//! `dG`, `dgg`) and `x` / `X`. Each delete is its own action: bound in
//! `bind_default_keys` to the natural vim sequence. Counts are read via
//! [`Editor::take_count`].

use std::sync::Arc;

use crate::cursor::Cursor;
use crate::editor::Register;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("delete_line", Arc::new(delete_line));
    reg.register("delete_line_down", Arc::new(delete_line_down));
    reg.register("delete_line_up", Arc::new(delete_line_up));
    reg.register("delete_word_forward", Arc::new(|ed| {
        delete_with_motion(ed, "word_forward", false)
    }));
    reg.register("delete_word_backward", Arc::new(|ed| {
        delete_with_motion(ed, "word_backward", false)
    }));
    reg.register("delete_word_end", Arc::new(|ed| {
        delete_with_motion(ed, "word_end", true)
    }));
    reg.register("delete_to_line_end", Arc::new(delete_to_line_end));
    reg.register("delete_to_line_start", Arc::new(delete_to_line_start));
    reg.register("delete_to_buffer_start", Arc::new(delete_to_buffer_start));
    reg.register("delete_to_buffer_end", Arc::new(delete_to_buffer_end));
    reg.register("delete_char", Arc::new(delete_char));
    reg.register("delete_char_before", Arc::new(delete_char_before));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    let bindings = [
        ("dd", "delete_line"),
        ("dj", "delete_line_down"),
        ("dk", "delete_line_up"),
        ("dw", "delete_word_forward"),
        ("db", "delete_word_backward"),
        ("de", "delete_word_end"),
        ("d$", "delete_to_line_end"),
        ("d0", "delete_to_line_start"),
        ("dgg", "delete_to_buffer_start"),
        ("dG", "delete_to_buffer_end"),
        ("D", "delete_to_line_end"),
        ("x", "delete_char"),
        ("X", "delete_char_before"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- Helpers ---------------------------------------------------------------

fn cursor_to_char(buf: &crate::buffer::Buffer, c: Cursor, tw: usize) -> usize {
    let line_start = buf.line_to_char(c.row);
    let line = buf.line_string(c.row);
    let byte = twidth::col_to_byte(&line, c.col, tw);
    line_start + line[..byte].chars().count()
}

fn place_cursor_at_char(editor: &mut Editor, char_idx: usize) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let total = b.len_chars();
    let idx = char_idx.min(total);
    let row = b.char_to_line(idx);
    let line_start = b.line_to_char(row);
    let off_chars = idx.saturating_sub(line_start);
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
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
}

/// Delete the char range `[lo, hi)`, yank it into the unnamed register, and
/// move the cursor to `lo`.
fn delete_range(editor: &mut Editor, lo: usize, hi: usize, linewise: bool) {
    if hi <= lo {
        return;
    }
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let removed = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        b.delete(lo..hi).removed
    };
    editor.unnamed_register = Register { text: removed, linewise };
    place_cursor_at_char(editor, lo);
}

// ---- Line-based deletes ----------------------------------------------------

fn delete_line(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let row = win.cursor.row;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let last_row = b.line_count().saturating_sub(1);
    let end_row = (row + count).min(last_row + 1);
    let lo = b.line_to_char(row);
    let hi = if end_row > last_row {
        b.len_chars()
    } else {
        b.line_to_char(end_row)
    };
    delete_range(editor, lo, hi, true);
}

fn delete_line_down(editor: &mut Editor) {
    // dj = delete current line + `count` lines below = (count + 1) lines.
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let row = win.cursor.row;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let last_row = b.line_count().saturating_sub(1);
    let end_row = (row + count + 1).min(last_row + 1);
    let lo = b.line_to_char(row);
    let hi = if end_row > last_row {
        b.len_chars()
    } else {
        b.line_to_char(end_row)
    };
    delete_range(editor, lo, hi, true);
}

fn delete_line_up(editor: &mut Editor) {
    // dk = delete current line + `count` lines above = (count + 1) lines,
    // anchored upward.
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let row = win.cursor.row;
    let top_row = row.saturating_sub(count);
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let last_row = b.line_count().saturating_sub(1);
    let end_row = (row + 1).min(last_row + 1);
    let lo = b.line_to_char(top_row);
    let hi = if end_row > last_row {
        b.len_chars()
    } else {
        b.line_to_char(end_row)
    };
    delete_range(editor, lo, hi, true);
}

// ---- Intra-line deletes ----------------------------------------------------

fn delete_to_line_end(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let lo = cursor_to_char(b, cursor, tw);
    let line_start = b.line_to_char(cursor.row);
    let line = b.line_string(cursor.row);
    let hi = line_start + line.chars().count();
    delete_range(editor, lo, hi, false);
}

fn delete_to_line_start(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let lo = b.line_to_char(cursor.row);
    let hi = cursor_to_char(b, cursor, tw);
    delete_range(editor, lo, hi, false);
}

fn delete_to_buffer_start(editor: &mut Editor) {
    let n = editor
        .pending_count_pre
        .take()
        .or_else(|| editor.pending_count_post.take());
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let top_row = n.map(|c| c.saturating_sub(1)).unwrap_or(0);
    let lo = b.line_to_char(top_row);
    let last_row = b.line_count().saturating_sub(1);
    let cur_end_row = (cursor.row + 1).min(last_row + 1);
    let hi = if cur_end_row > last_row {
        b.len_chars()
    } else {
        b.line_to_char(cur_end_row)
    };
    delete_range(editor, lo, hi, true);
}

fn delete_to_buffer_end(editor: &mut Editor) {
    let n = editor
        .pending_count_pre
        .take()
        .or_else(|| editor.pending_count_post.take());
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let last_row = b.line_count().saturating_sub(1);
    let bot_row = match n {
        Some(c) => c.saturating_sub(1).min(last_row),
        None => last_row,
    };
    let lo = b.line_to_char(cursor.row);
    let end_row = (bot_row + 1).min(last_row + 1);
    let hi = if end_row > last_row {
        b.len_chars()
    } else {
        b.line_to_char(end_row)
    };
    delete_range(editor, lo, hi, true);
}

// ---- Char deletes ----------------------------------------------------------

fn delete_char(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let lo = cursor_to_char(b, cursor, tw);
    // Cap deletion at end of current line so `x` doesn't pull in the newline.
    let line_start = b.line_to_char(cursor.row);
    let line = b.line_string(cursor.row);
    let line_end_char = line_start + line.chars().count();
    let hi = (lo + count).min(line_end_char);
    delete_range(editor, lo, hi, false);
    // Clamp cursor back if we ran off the end of the line.
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        if let Some(b) = editor.buffers.get(&buf_id) {
            let line = b.line_string(w.cursor.row);
            let lw = twidth::line_display_width(&line, tw);
            if w.cursor.col >= lw {
                w.cursor.col = lw.saturating_sub(1);
                w.cursor.sticky_col = w.cursor.col;
            }
        }
    }
}

fn delete_char_before(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let hi = cursor_to_char(b, cursor, tw);
    let line_start = b.line_to_char(cursor.row);
    let lo = hi.saturating_sub(count).max(line_start);
    delete_range(editor, lo, hi, false);
}

// ---- Motion-based deletes --------------------------------------------------

/// Run `motion_name` against the active cursor with the current count, then
/// delete from the original cursor to the new cursor. `inclusive` adds one
/// char past the motion target (used by `de`).
fn delete_with_motion(editor: &mut Editor, motion_name: &str, inclusive: bool) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let start = editor.windows.get(&win_id).unwrap().cursor;

    // Re-arm count so the motion sees it.
    editor.pending_count_pre = Some(count);
    editor.pending_count_post = None;

    let Some(action) = editor.actions.lookup(motion_name) else {
        return;
    };
    action(editor);

    let end = editor.windows.get(&win_id).unwrap().cursor;
    let tw = editor.config.options.tab_width;
    let b = editor.buffers.get(&buf_id).unwrap();
    let s = cursor_to_char(b, start, tw);
    let mut e = cursor_to_char(b, end, tw);
    if inclusive {
        e = e.saturating_add(1).min(b.len_chars());
    }
    let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
    delete_range(editor, lo, hi, false);
}
