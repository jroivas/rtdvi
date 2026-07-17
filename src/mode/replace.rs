//! Replace (overtype) mode, entered with `R`.
//!
//! Typed characters overwrite the character under the cursor instead of
//! inserting; once the cursor passes the old end of line, further characters
//! extend the line (as in insert mode). Backspace walks back through the
//! changes, restoring each overtyped character (or deleting appended ones).
//! `<Esc>` leaves to normal mode, stepping the cursor left one column like
//! insert mode. The whole session collapses into a single undo step.
//!
//! The per-character undo trail lives in [`Editor::replace_overwrite`]: one
//! entry pushed per typed character. `Some(orig)` means we overtyped an
//! existing character; `None` means we appended past the old line end or
//! inserted a newline — Backspace deletes those instead of restoring.

use crate::buffer::BufferId;
use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    let is_esc = key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.mods.contains(KeyMods::CTRL));
    if is_esc {
        editor.replace_overwrite.clear();
        editor.end_active_transaction();
        if let Some(w) = editor.active_window_mut() {
            if w.cursor.col > 0 {
                w.cursor.col -= 1;
                w.cursor.sticky_col = w.cursor.col;
            }
        }
        switch_mode(editor, ModeId::Normal);
        return;
    }
    match key.code {
        KeyCode::Char(c) if !key.mods.contains(KeyMods::CTRL) => overtype_char(editor, c),
        // Enter drops to the next line, inserting a break (vim does not
        // overtype across the line boundary). Recorded as an appended char so
        // Backspace rejoins the lines.
        KeyCode::Enter => {
            crate::mode::insert::insert_str(editor, "\n");
            editor.replace_overwrite.push(None);
        }
        KeyCode::Tab => overtype_char(editor, '\t'),
        KeyCode::Backspace => backspace(editor),
        _ => {}
    }
}

/// Insert `text` in overtype fashion (used for bracketed paste). Newlines
/// break the line; every other character overtypes.
pub(crate) fn replace_paste(editor: &mut Editor, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            crate::mode::insert::insert_str(editor, "\n");
            editor.replace_overwrite.push(None);
        } else {
            overtype_char(editor, c);
        }
    }
}

/// Overtype a single (non-newline) character at the cursor: replace the
/// character under the cursor, or append when the cursor is at/past the end
/// of the line. Advances the cursor by the character's display width.
fn overtype_char(editor: &mut Editor, c: char) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tw = editor.config.options.tab_width;

    let (char_idx, orig) = {
        let Some(b) = editor.buffers.get(&buf_id) else {
            return;
        };
        let line = b.line_string(cursor.row);
        let line_start = b.line_to_char(cursor.row);
        let cursor_byte = twidth::col_to_byte(&line, cursor.col, tw);
        let cursor_char_off = line[..cursor_byte].chars().count();
        // A character exists under the cursor only when the offset is inside
        // the line's own text (line_string excludes the trailing newline).
        let orig = line.chars().nth(cursor_char_off);
        (line_start + cursor_char_off, orig)
    };

    let edit = match editor.buffers.get_mut(&buf_id) {
        Some(b) => {
            if orig.is_some() {
                b.replace(char_idx..char_idx + 1, &c.to_string())
            } else {
                b.insert(char_idx, &c.to_string())
            }
        }
        None => return,
    };
    editor.replace_overwrite.push(orig);

    if let Some(w) = editor.windows.get_mut(&win_id) {
        let g = c.to_string();
        let wid = twidth::grapheme_width(&g, w.cursor.col, tw);
        w.cursor.col += wid.max(1);
        w.cursor.sticky_col = w.cursor.col;
    }
    emit_buffer_changed(editor, buf_id, edit);
}

/// Backspace: undo the most recent overtype/append. Restores the overtyped
/// character in place, deletes an appended one (rejoining lines when it was a
/// newline), and lands the cursor on the restored position. With nothing on
/// the stack the cursor just steps left without touching the text — matching
/// vim, which never lets Backspace destroy content you did not type over.
fn backspace(editor: &mut Editor) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tw = editor.config.options.tab_width;
    let char_idx = match editor.buffers.get(&buf_id) {
        Some(b) => b.cursor_to_char(cursor, tw),
        None => return,
    };
    if char_idx == 0 {
        return;
    }
    let prev = char_idx - 1;

    let edit = match editor.replace_overwrite.pop() {
        // Overtyped a real character — put the original back.
        Some(Some(orig)) => editor
            .buffers
            .get_mut(&buf_id)
            .map(|b| b.replace(prev..char_idx, &orig.to_string())),
        // Appended past the old line end (or inserted a newline) — remove it.
        Some(None) => editor.buffers.get_mut(&buf_id).map(|b| b.delete(prev..char_idx)),
        // Nothing left to undo: move the cursor left, leave the text alone.
        None => None,
    };

    if let Some(new_cursor) = editor.buffers.get(&buf_id).map(|b| b.char_to_cursor(prev, tw)) {
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor = new_cursor;
            w.cursor.sticky_col = new_cursor.col;
        }
    }
    if let Some(edit) = edit {
        emit_buffer_changed(editor, buf_id, edit);
    }
}

fn emit_buffer_changed(editor: &mut Editor, buffer: BufferId, edit: crate::buffer::Edit) {
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer, edit: &edit },
    );
}
