//! `y{motion}` operator family — yank into the unnamed register without
//! mutating the buffer or moving the cursor. Ranges mirror the corresponding
//! delete actions; cursor stays put (vim's default for most yanks).

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("yank_line", Arc::new(yank_line));
    reg.register("yank_line_down", Arc::new(yank_line_down));
    reg.register("yank_line_up", Arc::new(yank_line_up));
    reg.register("yank_word_forward", Arc::new(|ed| {
        yank_with_motion(ed, "word_forward", false)
    }));
    reg.register("yank_word_backward", Arc::new(|ed| {
        yank_with_motion(ed, "word_backward", false)
    }));
    reg.register("yank_word_end", Arc::new(|ed| {
        yank_with_motion(ed, "word_end", true)
    }));
    reg.register("yank_char_right", Arc::new(|ed| {
        yank_with_motion(ed, "move_right", false)
    }));
    reg.register("yank_char_left", Arc::new(|ed| {
        yank_with_motion(ed, "move_left", false)
    }));
    reg.register("yank_to_line_end", Arc::new(yank_to_line_end));
    reg.register("yank_to_line_start", Arc::new(yank_to_line_start));
    reg.register("yank_to_buffer_start", Arc::new(yank_to_buffer_start));
    reg.register("yank_to_buffer_end", Arc::new(yank_to_buffer_end));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    let bindings = [
        ("yy", "yank_line"),
        ("yj", "yank_line_down"),
        ("yk", "yank_line_up"),
        ("yw", "yank_word_forward"),
        ("yb", "yank_word_backward"),
        ("ye", "yank_word_end"),
        // Charwise horizontal: `yl` / `y<Space>` yank the char under the
        // cursor, `yh` the one before it. Count-aware (`3yl`).
        ("yl", "yank_char_right"),
        ("y<Space>", "yank_char_right"),
        ("yh", "yank_char_left"),
        ("y$", "yank_to_line_end"),
        ("y0", "yank_to_line_start"),
        ("ygg", "yank_to_buffer_start"),
        ("yG", "yank_to_buffer_end"),
        // Vim's `Y` is identical to `yy` (modern default; the legacy
        // "yank to end of line" can be remapped via TOML config).
        ("Y", "yank_line"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- Helpers ---------------------------------------------------------------

fn yank_range(editor: &mut Editor, lo: usize, hi: usize, linewise: bool) {
    if hi <= lo {
        return;
    }
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let text: String = match editor.buffers.get(&buf_id) {
        Some(b) => b.rope().slice(lo..hi).to_string(),
        None => return,
    };
    crate::registers::store(editor, text, linewise);
}

// ---- Line-based yanks ------------------------------------------------------

fn yank_line(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let row = win.cursor.row;
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let (lo, hi) = crate::action_util::line_span_chars(b, row, row + count);
    yank_range(editor, lo, hi, true);
}

fn yank_line_down(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, true);
}

fn yank_line_up(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, true);
}

// ---- Intra-line yanks ------------------------------------------------------

fn yank_to_line_end(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, false);
}

fn yank_to_line_start(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, false);
}

fn yank_to_buffer_start(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, true);
}

fn yank_to_buffer_end(editor: &mut Editor) {
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
    yank_range(editor, lo, hi, true);
}

// ---- Motion-based yanks ----------------------------------------------------

/// Run `motion_name` (with the current count) from the cursor, capture the
/// span from the original cursor to the motion target as the yank range,
/// then restore the cursor (yank doesn't move it).
fn yank_with_motion(editor: &mut Editor, motion_name: &str, inclusive: bool) {
    let count = editor.take_count();
    let Some((lo, hi, start)) =
        crate::action_util::motion_char_range(editor, motion_name, count, inclusive)
    else {
        return;
    };
    // Yank doesn't move the cursor — restore it.
    if let Some(w) = editor.active_window_mut() {
        w.cursor = start;
    }
    yank_range(editor, lo, hi, false);
}
