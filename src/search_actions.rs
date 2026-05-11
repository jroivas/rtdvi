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
    let buf = match editor.buffers.get(&buf_id) {
        Some(b) => b,
        None => return,
    };

    // We work over the buffer rendered as a flat string. For multi-MB files
    // this would be inefficient; v1 keeps it simple. M8 plan called for
    // `regex-cursor` to avoid the allocation, but `regex-cursor` is not yet
    // wired up — leaving as a known follow-up.
    let text = buf.rope().to_string();
    let line_starts: Vec<usize> = (0..buf.line_count())
        .map(|i| buf.line_to_char(i))
        .collect();

    let cursor_byte = {
        let line_start = buf.line_to_char(cursor.row);
        let line = buf.line_string(cursor.row);
        let byte_in_line = twidth::col_to_byte(&line, cursor.col, tw);
        char_to_byte(&text, line_start) + byte_in_line
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
        // Backward: find the last match before cursor; wrap if none.
        let before = &text[..cursor_byte];
        let mut last = None;
        for m in re.find_iter(before) {
            last = Some(m.start());
        }
        last.or_else(|| {
            let mut last_any = None;
            for m in re.find_iter(&text) {
                last_any = Some(m.start());
            }
            last_any
        })
    };

    let Some(byte) = target_byte else {
        editor.status_message = Some(format!(
            "E486: Pattern not found: {}",
            editor.search.last_pattern.as_deref().unwrap_or("")
        ));
        return;
    };

    // Convert byte offset back to (row, col).
    let (row, col_byte_in_line) = byte_to_row_col(&line_starts, &text, byte);
    let line = buf.line_string(row);
    let col = twidth::byte_to_col(&line, col_byte_in_line, tw);
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

fn byte_to_row_col(line_starts_chars: &[usize], text: &str, byte: usize) -> (usize, usize) {
    // line_starts_chars[i] is a *char* index. We need its byte equivalent.
    // Walk char_indices to find which line contains `byte`.
    let mut prev_line_byte = 0usize;
    let mut prev_row = 0usize;
    for (row, &char_start) in line_starts_chars.iter().enumerate() {
        let line_byte = char_to_byte(text, char_start);
        if line_byte > byte {
            return (prev_row, byte - prev_line_byte);
        }
        prev_line_byte = line_byte;
        prev_row = row;
    }
    (prev_row, byte - prev_line_byte)
}
