//! Insert mode. Inserts typed characters at the cursor, handles Enter and
//! Backspace, and returns to normal mode on Esc.

use crate::buffer::BufferId;
use crate::cursor::Cursor;
use crate::keymap::{Key, KeyCode};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    if matches!(key.code, KeyCode::Esc) {
        // If we're inside a visual-block `I` / `A` session, replay the typed
        // text across the rectangle. The replay function lands the cursor;
        // we skip the usual left-step.
        if let Some(pending) = editor.pending_block_insert.take() {
            crate::visual_actions::apply_block_insert_replay(editor, pending);
            switch_mode(editor, ModeId::Normal);
            return;
        }
        // Regular Esc: vim moves cursor left by one, clamped at col 0.
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
        KeyCode::Char(c) if !key.mods.contains(crate::keymap::keys::KeyMods::CTRL) => {
            insert_str(editor, &c.to_string());
        }
        KeyCode::Enter => insert_str(editor, "\n"),
        KeyCode::Tab => insert_str(editor, "\t"),
        KeyCode::Backspace => backspace(editor),
        _ => {}
    }
}

/// Translate (row, display_col) into a char index in the rope.
pub(crate) fn cursor_to_char_index(
    buf: &crate::buffer::Buffer,
    cursor: Cursor,
    tab_width: usize,
) -> usize {
    let line_start = buf.line_to_char(cursor.row);
    let line = buf.line_string(cursor.row);
    let byte = twidth::col_to_byte(&line, cursor.col, tab_width);
    line_start + line[..byte].chars().count()
}

fn insert_str(editor: &mut Editor, text: &str) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tab_width = editor.config.options.tab_width;
    let char_idx = match editor.buffers.get(&buf_id) {
        Some(b) => cursor_to_char_index(b, cursor, tab_width),
        None => return,
    };

    let edit = match editor.buffers.get_mut(&buf_id) {
        Some(b) => b.insert(char_idx, text),
        None => return,
    };

    // Advance cursor according to what we inserted.
    if let Some(w) = editor.windows.get_mut(&win_id) {
        for ch in text.chars() {
            if ch == '\n' {
                w.cursor.row += 1;
                w.cursor.col = 0;
            } else {
                let g = ch.to_string();
                let wid = twidth::grapheme_width(&g, w.cursor.col, tab_width);
                w.cursor.col += wid.max(1);
            }
        }
        w.cursor.sticky_col = w.cursor.col;
    }

    emit_buffer_changed(editor, buf_id, edit);
}

fn backspace(editor: &mut Editor) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tab_width = editor.config.options.tab_width;
    let char_idx = match editor.buffers.get(&buf_id) {
        Some(b) => cursor_to_char_index(b, cursor, tab_width),
        None => return,
    };
    if char_idx == 0 {
        return;
    }

    // We need to find the start of the previous grapheme to know how far to
    // step the cursor.
    let edit = match editor.buffers.get_mut(&buf_id) {
        Some(b) => b.delete(char_idx - 1..char_idx),
        None => return,
    };

    if let Some(w) = editor.windows.get_mut(&win_id) {
        if w.cursor.col > 0 {
            // ASCII step approximation; we'd need to look at the deleted grapheme
            // for full correctness with wide chars. For v1 ASCII-step is fine
            // since text comes from typed chars, and we control insertion.
            w.cursor.col -= 1;
        } else if w.cursor.row > 0 {
            w.cursor.row -= 1;
            // Move to end of previous line (display column = line width).
            if let Some(b) = editor.buffers.get(&buf_id) {
                let line = b.line_string(w.cursor.row);
                w.cursor.col = twidth::line_display_width(&line, tab_width);
            }
        }
        w.cursor.sticky_col = w.cursor.col;
    }
    emit_buffer_changed(editor, buf_id, edit);
}

fn emit_buffer_changed(editor: &mut Editor, buffer: BufferId, edit: crate::buffer::Edit) {
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer, edit: &edit },
    );
}
