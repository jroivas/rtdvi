//! Actions for entering the search prompt and stepping through matches.

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("search_forward", Arc::new(|ed| start_search(ed, true)));
    reg.register("search_backward", Arc::new(|ed| start_search(ed, false)));
    reg.register("search_next", Arc::new(|ed| {
        let fwd = ed.search.direction_forward;
        jump_match(ed, fwd, false);
    }));
    reg.register("search_prev", Arc::new(|ed| {
        let fwd = !ed.search.direction_forward;
        jump_match(ed, fwd, false);
    }));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    let bindings = [
        ("/", "search_forward"),
        ("?", "search_backward"),
        ("n", "search_next"),
        ("N", "search_prev"),
    ];
    for (seq, action) in bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
}

fn start_search(editor: &mut Editor, forward: bool) {
    editor.search.clear_prompt();
    editor.search.direction_forward = forward;
    switch_mode(editor, ModeId::Search);
}

/// Jump the cursor to the next match of the active pattern. If `start_at_cursor`
/// is false, we search from one past the cursor (the usual `n` behaviour).
pub fn jump_match(editor: &mut Editor, forward: bool, start_at_cursor: bool) {
    let Some(re) = editor.search.pattern.clone() else {
        editor.status_message = Some("E: no search pattern".into());
        return;
    };
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tw = editor.config.options.tab_width;
    // Large files open via mmap with an empty rope; searching needs the full
    // text and the rope's byte/line index, so build it once here (idempotent
    // for already-loaded small files).
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        b.materialize();
    }
    let buf = match editor.buffers.get(&buf_id) {
        Some(b) => b,
        None => return,
    };

    // We work over the buffer rendered as a flat string. All byte<->line
    // conversions go through the rope (O(log n)) — never via linear char-index
    // scans, which were O(n²) over the whole file and made backward search on
    // multi-MB files hang for many seconds.
    let rope = buf.rope();
    let text = rope.to_string();

    let cursor_byte = {
        let row = cursor.row.min(rope.len_lines().saturating_sub(1));
        let line = buf.line_string(row);
        let byte_in_line = twidth::col_to_byte(&line, cursor.col, tw);
        (rope.line_to_byte(row) + byte_in_line).min(text.len())
    };

    let target_byte: Option<usize> = if forward {
        let start = if start_at_cursor {
            cursor_byte
        } else {
            // Advance one byte past current cursor so n doesn't stick.
            cursor_byte + text[cursor_byte..].chars().next().map_or(0, char::len_utf8)
        };
        re.find(&text[start..])
            .map(|m| start + m.start())
            .or_else(|| re.find(&text).map(|m| m.start()))
    } else {
        // Backward: the last match starting before the cursor; wrap if none.
        let before = &text[..cursor_byte];
        re.find_iter(before)
            .last()
            .map(|m| m.start())
            .or_else(|| re.find_iter(&text).last().map(|m| m.start()))
    };

    let Some(byte) = target_byte else {
        editor.status_message = Some(format!(
            "E486: Pattern not found: {}",
            editor.search.last_pattern.as_deref().unwrap_or("")
        ));
        return;
    };

    // Convert byte offset back to (row, col) via the rope (O(log n)).
    let row = rope.byte_to_line(byte);
    let col_byte_in_line = byte - rope.line_to_byte(row);
    let line = buf.line_string(row);
    let col = twidth::byte_to_col(&line, col_byte_in_line, tw);
    // Record the position we're about to leave so `<C-o>` can return.
    // Coalescing in `Jumplist::record` swallows duplicate consecutive
    // `n`/`N` presses that stay on the same line.
    editor.jumplist_record_here();
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
}

