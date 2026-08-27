//! `d{motion}` operator family, plus line-based variants (`dd`, `dj`, `dk`,
//! `dG`, `dgg`) and `x` / `X`. Each delete is its own action: bound in
//! `bind_default_keys` to the natural vim sequence. Counts are read via
//! [`Editor::take_count`].

use std::sync::Arc;

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
        // Charwise horizontal, mirroring `yl`/`yh`. `dl`/`d<Space>` delete the
        // char under the cursor (like `x`), `dh` the one before (like `X`).
        ("dl", "delete_char"),
        ("d<Space>", "delete_char"),
        ("dh", "delete_char_before"),
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

fn place_cursor_at_char(editor: &mut Editor, char_idx: usize) {
    let tw = editor.config.options.tab_width;
    let Some(cursor) = editor.active_buffer().map(|b| b.char_to_cursor(char_idx, tw)) else {
        return;
    };
    if let Some(w) = editor.active_window_mut() {
        w.cursor = cursor;
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
    let edit = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        b.delete(lo..hi)
    };
    crate::registers::store(editor, edit.removed.clone(), linewise);
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );
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
    let saved_col = win.cursor.col;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let (lo, hi) = crate::action_util::line_span_chars(b, row, row + count);
    delete_range(editor, lo, hi, true);
    // place_cursor_at_char(lo) always lands at col 0 because lo is a
    // line-start offset. Restore the column the cursor was on before the
    // delete, clamped to the new line's display width.
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let cursor_row = match editor.windows.get(&win_id) {
        Some(w) => w.cursor.row,
        None => return,
    };
    let cap = match editor.buffers.get(&buf_id) {
        Some(b) => twidth::line_display_width(&b.line_string(cursor_row), tw).saturating_sub(1),
        None => return,
    };
    if let Some(win) = editor.windows.get_mut(&win_id) {
        win.cursor.col = saved_col.min(cap);
        win.cursor.sticky_col = win.cursor.col;
    }
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
    let (lo, hi) = crate::action_util::line_span_chars(b, row, row + count + 1);
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
    let (lo, hi) = crate::action_util::line_span_chars(b, top_row, row + 1);
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
    let lo = b.cursor_to_char(cursor, tw);
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
    let hi = b.cursor_to_char(cursor, tw);
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
    let (lo, hi) = crate::action_util::line_span_chars(b, top_row, cursor.row + 1);
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
    let (lo, hi) = crate::action_util::line_span_chars(b, cursor.row, bot_row + 1);
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
    let lo = b.cursor_to_char(cursor, tw);
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
    let hi = b.cursor_to_char(cursor, tw);
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
    let Some((lo, hi, _start)) =
        crate::action_util::motion_char_range(editor, motion_name, count, inclusive)
    else {
        return;
    };
    delete_range(editor, lo, hi, false);
}
