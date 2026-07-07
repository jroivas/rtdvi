//! Insert mode. Inserts typed characters at the cursor, handles Enter and
//! Backspace, and returns to normal mode on Esc.

use crate::buffer::BufferId;
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
        // Regular Esc: if autoindent left a blank line, strip its whitespace
        // (vim discards auto-indent you never typed on). Then close the insert
        // transaction and step the cursor left as vim does.
        strip_blank_autoindent(editor);
        editor.auto_indent_blank = None;
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
    // Paste mode: insert everything verbatim — no brace-dedent, no autoindent
    // on Enter, literal tab on Tab. Mirrors vim's `:set paste`.
    let paste = editor.config.options.paste;
    match key.code {
        KeyCode::Char(c) if !key.mods.contains(crate::keymap::keys::KeyMods::CTRL) => {
            match c {
                '{' if !paste => handle_brace(editor, '{'),
                '}' if !paste => handle_brace(editor, '}'),
                _ => insert_str(editor, &c.to_string()),
            }
            // Typing real (non-whitespace) content commits the auto-indented
            // line, so it is no longer a candidate for blank-line stripping.
            if !matches!(c, ' ' | '\t') {
                editor.auto_indent_blank = None;
            }
        }
        KeyCode::Enter => {
            if paste {
                insert_str(editor, "\n");
                editor.auto_indent_blank = None;
            } else {
                let indent = autoindent_for_enter(editor); // &mut Editor — sequential, no conflict
                // Pressing Enter on a still-blank auto-indented line discards
                // that indent, so we don't leave a whitespace-only line behind.
                strip_blank_autoindent(editor);
                insert_str(editor, &format!("\n{indent}"));
                record_blank_autoindent(editor, !indent.is_empty());
            }
        }
        KeyCode::Tab => {
            let text = if paste { "\t".to_string() } else { tab_insertion(editor) };
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

/// Insert pasted `text`, then land the cursor on the last *visible*
/// character of the paste (skipping trailing newlines) — matching vim's
/// `p`, instead of sitting one column past the end like normal typing.
pub(crate) fn insert_paste(editor: &mut Editor, text: &str) {
    insert_str(editor, text);

    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tab_width = editor.config.options.tab_width;
    let Some(buf) = editor.buffers.get(&buf_id) else {
        return;
    };
    // `insert_str` left the cursor one past the last inserted char. Step back
    // over the final char plus any trailing newlines to reach the last
    // visible character of the paste.
    let end_char = buf.cursor_to_char(cursor, tab_width);
    let trailing_newlines = text.chars().rev().take_while(|&c| c == '\n').count();
    let target = end_char.saturating_sub(1 + trailing_newlines);
    let new_cursor = buf.char_to_cursor(target, tab_width);

    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor = new_cursor;
    }
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
        Some(b) => b.cursor_to_char(cursor, tab_width),
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
        Some(b) => b.cursor_to_char(cursor, tab_width),
        None => return,
    };
    if char_idx == 0 {
        return;
    }

    // Smart backspace: when the cursor is inside leading whitespace, snap
    // to the previous tab stop instead of deleting one space at a time.
    // Falls back to (1, col-1) for normal characters.
    let (delete_n, new_col) = if cursor.col > 0
        && editor.config.options.smartindent
        && !editor.config.options.paste
    {
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

/// Record that the current cursor line is a freshly auto-indented, otherwise
/// blank line, so a subsequent Enter/Esc that leaves it blank can strip the
/// indent. `has_indent` is false when no indent was inserted (nothing to strip).
fn record_blank_autoindent(editor: &mut Editor, has_indent: bool) {
    if !has_indent {
        editor.auto_indent_blank = None;
        return;
    }
    let cursor = editor.active_window().map(|w| (w.buffer, w.cursor.row));
    editor.auto_indent_blank = cursor;
}

/// If autoindent left a blank (whitespace-only) line pending and it is still
/// blank, delete that leading whitespace so no trailing indent lingers. Mirrors
/// vim's `autoindent`: indentation you never typed on is discarded when you
/// leave the line. Real content typed on the line cancels the strip.
fn strip_blank_autoindent(editor: &mut Editor) {
    let Some((buf_id, row)) = editor.auto_indent_blank else {
        return;
    };
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    if row >= b.line_count() {
        return;
    }
    let line = b.line_string(row);
    let n = line.chars().count();
    if n == 0 || !line.chars().all(|c| c == ' ' || c == '\t') {
        return; // empty already, or real content typed — leave it be
    }
    let start = b.line_to_char(row);
    let Some(b) = editor.buffers.get_mut(&buf_id) else {
        return;
    };
    let edit = b.delete(start..start + n);
    for w in editor.windows.values_mut() {
        if w.buffer == buf_id && w.cursor.row == row {
            w.cursor.col = 0;
            w.cursor.sticky_col = 0;
        }
    }
    emit_buffer_changed(editor, buf_id, edit);
}

/// Compute the indentation string to prepend after inserting a newline at the
/// current cursor position. Delegates to a registered plugin indent provider
/// when one is available for the buffer's filetype; falls back to the built-in
/// smartindent otherwise.
fn autoindent_for_enter(editor: &mut Editor) -> String {
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

    // If autoindent is off, skip everything.
    if !editor.config.options.autoindent {
        return String::new();
    }

    // Try a registered plugin indent provider first.
    // Only call the plugin when the cursor is at EOL (same condition as smartindent).
    #[cfg(feature = "plugins")]
    if at_eol && editor.config.options.smartindent {
        if let Some(indent) =
            crate::plugin::call_plugin_indent(editor, filetype, buf_id, cursor.row)
        {
            return indent;
        }
    }

    // Re-borrow after the mutable plugin call.
    let Some(buf) = editor.buffers.get(&buf_id) else {
        return String::new();
    };
    let line = buf.line_string(cursor.row);
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
