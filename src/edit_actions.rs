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
        ("u", "undo"),
        ("<C-r>", "redo"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
}

fn enter_insert_before(editor: &mut Editor) {
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
    switch_mode(editor, ModeId::Insert);
}

fn enter_insert_line_start(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
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
    switch_mode(editor, ModeId::Insert);
    crate::mode::insert::handle_key(editor, crate::keymap::Key::new(crate::keymap::KeyCode::Enter));
}

fn open_line_above(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
    switch_mode(editor, ModeId::Insert);
    crate::mode::insert::handle_key(editor, crate::keymap::Key::new(crate::keymap::KeyCode::Enter));
    // After inserting '\n' at col 0, cursor went to (row+1, 0). We want to be
    // on the new empty line, which is now `row` (the original). Move up.
    if let Some(w) = editor.active_window_mut() {
        if w.cursor.row > 0 {
            w.cursor.row -= 1;
        }
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
