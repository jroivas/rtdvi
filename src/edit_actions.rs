//! Actions that transition into insert mode or perform simple edits.
//!
//! Kept separate from `motion.rs` so motions stay pure (no buffer mutation).

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("enter_insert_before", Arc::new(enter_insert_before));
    reg.register("enter_insert_after", Arc::new(enter_insert_after));
    reg.register("enter_insert_line_start", Arc::new(enter_insert_line_start));
    reg.register("enter_insert_line_end", Arc::new(enter_insert_line_end));
    reg.register("open_line_below", Arc::new(open_line_below));
    reg.register("open_line_above", Arc::new(open_line_above));
    reg.register("join_lines", Arc::new(join_lines));
    reg.register("undo", Arc::new(undo_action));
    reg.register("redo", Arc::new(redo_action));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    let bindings = [
        ("i", "enter_insert_before"),
        ("a", "enter_insert_after"),
        ("I", "enter_insert_line_start"),
        ("A", "enter_insert_line_end"),
        ("o", "open_line_below"),
        ("O", "open_line_above"),
        ("J", "join_lines"),
        ("u", "undo"),
        ("<C-r>", "redo"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
}

fn enter_insert_before(editor: &mut Editor) {
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

fn enter_insert_after(editor: &mut Editor) {
    // Move cursor right by 1 (past end of line is allowed in insert mode).
    let tab_width = editor.config.options.tab_width;
    if let Some(w) = editor.active_window_mut() {
        // Read the line width via active buffer.
        let row = w.cursor.row;
        let col = w.cursor.col;
        let _ = (row, col);
    }
    let buf_id = match editor.active_window() {
        Some(w) => w.buffer,
        None => return,
    };
    let line_width = match editor.buffers.get(&buf_id) {
        Some(b) => {
            let row = editor.active_window().unwrap().cursor.row;
            twidth::line_display_width(&b.line_string(row), tab_width)
        }
        None => return,
    };
    if let Some(w) = editor.active_window_mut() {
        if w.cursor.col < line_width {
            w.cursor.col += 1;
            w.cursor.sticky_col = w.cursor.col;
        }
    }
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

fn enter_insert_line_start(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

fn enter_insert_line_end(editor: &mut Editor) {
    let tab_width = editor.config.options.tab_width;
    let buf_id = match editor.active_window() {
        Some(w) => w.buffer,
        None => return,
    };
    let line_width = match editor.buffers.get(&buf_id) {
        Some(b) => {
            let row = editor.active_window().unwrap().cursor.row;
            twidth::line_display_width(&b.line_string(row), tab_width)
        }
        None => return,
    };
    if let Some(w) = editor.active_window_mut() {
        w.cursor.col = line_width;
        w.cursor.sticky_col = w.cursor.col;
    }
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

fn open_line_below(editor: &mut Editor) {
    // Move cursor to end of current line then insert '\n' (which puts us on a
    // new empty line below).
    let tab_width = editor.config.options.tab_width;
    let win_id = match editor.tabs.get(editor.active_tab) {
        Some(t) => t.active,
        None => return,
    };
    let (buf_id, row) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor.row),
        None => return,
    };
    let line_width = match editor.buffers.get(&buf_id) {
        Some(b) => twidth::line_display_width(&b.line_string(row), tab_width),
        None => return,
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.col = line_width;
        w.cursor.sticky_col = w.cursor.col;
    }
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
    crate::mode::insert::handle_key(editor, crate::keymap::Key::new(crate::keymap::KeyCode::Enter));
}

fn open_line_above(editor: &mut Editor) {
    let tab_width = editor.config.options.tab_width;
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else { return; };
    let (buf_id, row) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor.row),
        None => return,
    };

    // Copy the current line's leading whitespace — no smartindent for O.
    let indent = editor
        .buffers
        .get(&buf_id)
        .map(|b| crate::autoindent::leading_whitespace(&b.line_string(row)).to_string())
        .unwrap_or_default();

    // Move cursor to col 0 and enter insert mode.
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);

    // Insert `indent + "\n"` at col 0. This inserts a new line *above* the
    // current one that already contains the indent, without corrupting the
    // existing line's leading whitespace.
    let to_insert = format!("{indent}\n");
    crate::mode::insert::insert_str(editor, &to_insert);

    // Cursor is now at (row+1, 0). Step back up to the newly-created line.
    if let Some(w) = editor.active_window_mut() {
        if w.cursor.row > 0 {
            w.cursor.row -= 1;
            let col = twidth::line_display_width(&indent, tab_width);
            w.cursor.col = col;
            w.cursor.sticky_col = col;
        }
    }
}

/// `J` — join the current line with the line(s) below, vim-style. A count
/// `NJ` joins N lines (`J` and `2J` both join two). Leading whitespace of
/// each joined line is removed and a single space is inserted at the seam —
/// except when the running line already ends in whitespace, the joined text
/// starts with `)`, or it is empty. The cursor lands on the last seam.
fn join_lines(editor: &mut Editor) {
    let count = editor.take_count();
    let tw = editor.config.options.tab_width;
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, row0) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor.row),
        None => return,
    };

    let (joined, target_off, start, end) = {
        let Some(b) = editor.buffers.get(&buf_id) else {
            return;
        };
        let last_line = b.line_count().saturating_sub(1);
        if row0 >= last_line {
            return; // nothing below to join
        }
        // `J`/`2J` join two lines (append one); `NJ` appends N-1.
        let appends = count.saturating_sub(1).max(1);
        let last_row = (row0 + appends).min(last_line);

        let mut acc = b.line_string(row0);
        let mut target_off = acc.chars().count();
        for r in (row0 + 1)..=last_row {
            let next = b.line_string(r);
            let stripped = next.trim_start_matches([' ', '\t']);
            target_off = acc.chars().count();
            acc.push_str(join_separator(&acc, stripped));
            acc.push_str(stripped);
        }
        let start = b.line_to_char(row0);
        let end = b.line_to_char(last_row) + b.line_string(last_row).chars().count();
        (acc, target_off, start, end)
    };

    let edit = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        b.replace(start..end, &joined)
    };
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );

    // Park the cursor on the seam of the final join (the inserted space, or
    // the first joined-in character when no space was added).
    if let Some(b) = editor.buffers.get(&buf_id) {
        let line = b.line_string(row0);
        let byte = line
            .char_indices()
            .nth(target_off)
            .map(|(bo, _)| bo)
            .unwrap_or(line.len());
        let max_col = twidth::line_display_width(&line, tw).saturating_sub(1);
        let col = twidth::byte_to_col(&line, byte, tw).min(max_col);
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor.row = row0;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
        }
    }
}

/// The seam inserted between the running joined line `acc` and the next
/// line's `stripped` (leading-whitespace-removed) text. A single space,
/// unless that would be redundant or unwanted.
fn join_separator(acc: &str, stripped: &str) -> &'static str {
    if acc.is_empty()
        || acc.ends_with(' ')
        || acc.ends_with('\t')
        || stripped.is_empty()
        || stripped.starts_with(')')
    {
        ""
    } else {
        " "
    }
}

fn undo_action(editor: &mut Editor) {
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let edit_opt = editor.buffers.get_mut(&buf_id).and_then(|b| b.undo());
    let Some(edit) = edit_opt else { return; };

    // Compute the cursor target while only `editor.buffers` is borrowed.
    let tab_width = editor.config.options.tab_width;
    let target = editor.buffers.get(&buf_id).map(|b| {
        let row = b.char_to_line(edit.range.start);
        let line_start = b.line_to_char(row);
        let off_in_line = edit.range.start.saturating_sub(line_start);
        let line = b.line_string(row);
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_in_line {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, tab_width);
        (row, col)
    });
    if let (Some((row, col)), Some(w)) = (target, editor.active_window_mut()) {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );
}

fn redo_action(editor: &mut Editor) {
    let Some(buf_id) = editor.active_buffer_id() else {
        return;
    };
    let edit_opt = editor.buffers.get_mut(&buf_id).and_then(|b| b.redo());
    if let Some(edit) = edit_opt {
        crate::event::emit(
            editor,
            crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
        );
    }
}
