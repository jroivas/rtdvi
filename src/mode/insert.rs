//! Insert mode. Inserts typed characters at the cursor, handles Enter and
//! Backspace, and returns to normal mode on Esc.

use crate::buffer::BufferId;
use crate::cursor::Cursor;
use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    let is_esc = key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.mods.contains(KeyMods::CTRL));
    if is_esc {
        // If we're inside a visual-block `I` / `A` session, replay the typed
        // text across the rectangle. The replay function lands the cursor;
        // we skip the usual left-step.
        if let Some(pending) = editor.pending_block_insert.take() {
            crate::visual_actions::apply_block_insert_replay(editor, pending);
            // Close the transaction opened by block_insert_at_left /
            // block_append_at_right so the whole block change collapses
            // into one undo step.
            editor.end_active_transaction();
            switch_mode(editor, ModeId::Normal);
            return;
        }
        // Regular Esc: close any open transaction (insert session), then
        // step the cursor left as vim does.
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
        KeyCode::Char(c) if !key.mods.contains(crate::keymap::keys::KeyMods::CTRL) => {
            match c {
                '{' => handle_brace(editor, '{'),
                '}' => handle_brace(editor, '}'),
                _ => insert_str(editor, &c.to_string()),
            }
        }
        KeyCode::Enter => {
            let indent = autoindent_for_enter(editor);
            insert_str(editor, &format!("\n{indent}"));
        }
        KeyCode::Tab => {
            let text = tab_insertion(editor);
            insert_str(editor, &text);
        }
        // Shift+Tab always inserts a literal tab, even when `expandtab` is on.
        // Escape hatch for files that genuinely need a tab character.
        KeyCode::BackTab => insert_str(editor, "\t"),
        KeyCode::Backspace => backspace(editor),
        _ => {}
    }
}

/// Pick what `<Tab>` should insert: spaces up to the next tab stop when
/// `expandtab` is on, otherwise a literal `\t`. Computed from the active
/// window's cursor display column so the result lands on a `tab_width`
/// boundary regardless of how short the indent already is.
fn tab_insertion(editor: &Editor) -> String {
    let opts = &editor.config.options;
    if !opts.expandtab {
        return "\t".into();
    }
    let tw = opts.tab_width.max(1);
    let col = editor.active_window().map(|w| w.cursor.col).unwrap_or(0);
    let n = tw - (col % tw);
    " ".repeat(n)
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

pub(crate) fn insert_str(editor: &mut Editor, text: &str) {
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

    // Smart backspace: when the cursor is inside leading whitespace, snap
    // to the previous tab stop instead of deleting one space at a time.
    // Falls back to (1, col-1) for normal characters.
    let (delete_n, new_col) = if cursor.col > 0 && editor.config.options.smartindent {
        match editor.buffers.get(&buf_id) {
            Some(b) => {
                let line = b.line_string(cursor.row);
                let byte = twidth::col_to_byte(&line, cursor.col, tab_width);
                let before = &line[..byte];
                crate::autoindent::smart_backspace(before, cursor.col, tab_width)
            }
            None => (1, cursor.col.saturating_sub(1)),
        }
    } else {
        (1, cursor.col.saturating_sub(1))
    };

    let edit = match editor.buffers.get_mut(&buf_id) {
        Some(b) => b.delete(char_idx - delete_n..char_idx),
        None => return,
    };

    if let Some(w) = editor.windows.get_mut(&win_id) {
        if cursor.col > 0 {
            w.cursor.col = new_col;
        } else if w.cursor.row > 0 {
            w.cursor.row -= 1;
            if let Some(b) = editor.buffers.get(&buf_id) {
                let line = b.line_string(w.cursor.row);
                w.cursor.col = twidth::line_display_width(&line, tab_width);
            }
        }
        w.cursor.sticky_col = w.cursor.col;
    }
    emit_buffer_changed(editor, buf_id, edit);
}

/// Compute the indentation string to prepend after inserting a newline at the
/// current cursor position.
fn autoindent_for_enter(editor: &Editor) -> String {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return String::new();
    };
    let Some(w) = editor.windows.get(&win_id) else {
        return String::new();
    };
    let (buf_id, cursor) = (w.buffer, w.cursor);
    let Some(buf) = editor.buffers.get(&buf_id) else {
        return String::new();
    };
    let tab_width = editor.config.options.tab_width;
    let line = buf.line_string(cursor.row);
    let line_width = twidth::line_display_width(&line, tab_width);
    let at_eol = cursor.col >= line_width;
    let filetype = editor.syntax_for(buf_id).filetype;
    let opts = &editor.config.options;
    crate::autoindent::next_line_indent(
        &line,
        at_eol,
        filetype,
        tab_width,
        opts.expandtab,
        opts.autoindent,
        opts.smartindent,
    )
}

/// When `{` or `}` is typed in a C-family language on a pure-whitespace
/// line, dedent by one level before inserting so the brace aligns with
/// the surrounding block keyword.
fn handle_brace(editor: &mut Editor, ch: char) {
    let ch_str = ch.to_string();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        insert_str(editor, &ch_str);
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => {
            insert_str(editor, &ch_str);
            return;
        }
    };
    let tab_width = editor.config.options.tab_width;
    let (before_cursor, filetype) = match editor.buffers.get(&buf_id) {
        Some(b) => {
            let line = b.line_string(cursor.row);
            let byte = twidth::col_to_byte(&line, cursor.col, tab_width);
            (line[..byte].to_string(), editor.syntax_for(buf_id).filetype)
        }
        None => {
            insert_str(editor, &ch_str);
            return;
        }
    };

    let n = if editor.config.options.smartindent {
        crate::autoindent::brace_dedent(&before_cursor, filetype, tab_width)
    } else {
        0
    };
    if n > 0 {
        let lo = editor
            .buffers
            .get(&buf_id)
            .map(|b| b.line_to_char(cursor.row))
            .unwrap_or(0);
        let edit = match editor.buffers.get_mut(&buf_id) {
            Some(b) => b.delete(lo..lo + n),
            None => {
                insert_str(editor, &ch_str);
                return;
            }
        };
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor.col = w.cursor.col.saturating_sub(tab_width.max(1));
            w.cursor.sticky_col = w.cursor.col;
        }
        emit_buffer_changed(editor, buf_id, edit);
    }

    insert_str(editor, &ch_str);
}

fn emit_buffer_changed(editor: &mut Editor, buffer: BufferId, edit: crate::buffer::Edit) {
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer, edit: &edit },
    );
}
