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
    // Remember where we started so incremental preview searches from a stable
    // origin and cancelling the prompt can snap us back.
    editor.search.origin = editor.tabs.get(editor.active_tab).map(|t| t.active).and_then(|id| {
        editor.windows.get(&id).map(|w| crate::search::SearchOrigin {
            row: w.cursor.row,
            col: w.cursor.col,
            top_line: w.top_line,
            left_col: w.left_col,
        })
    });
    switch_mode(editor, ModeId::Search);
}

/// Incremental-search preview: while typing at the `/`/`?` prompt, jump the
/// cursor to the first match of the in-progress pattern, measured from the
/// recorded origin. An empty/invalid pattern or no match snaps the view back
/// to the origin. No jumplist entry is recorded — that happens on submit.
pub fn incsearch_preview(editor: &mut Editor) {
    let Some(origin) = editor.search.origin else {
        return;
    };
    let origin_cursor = crate::cursor::Cursor::new(origin.row, origin.col);
    let forward = editor.search.direction_forward;
    let re = editor.search.prompt_re.clone();
    let target = re
        .as_ref()
        .and_then(|re| find_match(editor, re, origin_cursor, forward, true));

    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    match target {
        Some((row, col)) => {
            if let Some(w) = editor.windows.get_mut(&win_id) {
                w.cursor.row = row;
                w.cursor.col = col;
                w.cursor.sticky_col = col;
            }
            // Center the previewed match, matching goto and the committed jump.
            center_active(editor);
        }
        None => {
            // Empty prompt / no match: restore the original view exactly.
            if let Some(w) = editor.windows.get_mut(&win_id) {
                w.cursor = origin_cursor;
                w.top_line = origin.top_line;
                w.left_col = origin.left_col;
            }
        }
    }
}

/// Jump the cursor to the next match of the active pattern. If `start_at_cursor`
/// is false, we search from one past the cursor (the usual `n` behaviour).
pub fn jump_match(editor: &mut Editor, forward: bool, start_at_cursor: bool) {
    let Some(re) = editor.search.pattern.clone() else {
        editor.status_message = Some("E: no search pattern".into());
        return;
    };
    let Some(cursor) = editor.active_window().map(|w| w.cursor) else {
        return;
    };
    match find_match(editor, &re, cursor, forward, start_at_cursor) {
        Some((row, col)) => {
            // Record the position we're about to leave so `<C-o>` can return.
            // Coalescing in `Jumplist::record` swallows duplicate consecutive
            // `n`/`N` presses that stay on the same line.
            editor.jumplist_record_here();
            if let Some(w) = editor.active_window_mut() {
                w.cursor.row = row;
                w.cursor.col = col;
                w.cursor.sticky_col = col;
            }
            // Center the match in the view (clamped near the buffer end),
            // matching a counted `{n}G` jump.
            center_active(editor);
        }
        None => {
            editor.status_message = Some(format!(
                "E486: Pattern not found: {}",
                editor.search.last_pattern.as_deref().unwrap_or("")
            ));
        }
    }
}

/// Center the active window on its cursor, clamped so it never scrolls past
/// the buffer end. Shared landing behaviour for search jumps and incremental
/// preview — same as a counted `{n}G`.
fn center_active(editor: &mut Editor) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let last = editor
        .windows
        .get(&win_id)
        .map(|w| w.buffer)
        .and_then(|b| editor.buffers.get(&b))
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0);
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.center_on_cursor_clamped(last);
    }
}

/// Find the start of the first match of `re` from `origin`, in `forward` (else
/// backward) direction, wrapping around the buffer. `inclusive` includes a
/// match starting exactly at `origin` (submit / incremental preview); `n`/`N`
/// pass `false` to step past the current position. Returns the match as a
/// `(row, col)` display position, or `None` when there is no match. Reads the
/// buffer but never moves the cursor or touches the jumplist.
fn find_match(
    editor: &mut Editor,
    re: &regex::Regex,
    origin: crate::cursor::Cursor,
    forward: bool,
    inclusive: bool,
) -> Option<(usize, usize)> {
    let win_id = editor.tabs.get(editor.active_tab).map(|t| t.active)?;
    let buf_id = editor.windows.get(&win_id).map(|w| w.buffer)?;
    let tw = editor.config.options.tab_width;
    // Large files open via mmap with an empty rope; searching needs the full
    // text and the rope's byte/line index, so build it once here (idempotent
    // for already-loaded small files).
    if let Some(b) = editor.buffers.get_mut(&buf_id) {
        b.materialize();
    }
    let buf = editor.buffers.get(&buf_id)?;

    // We work over the buffer rendered as a flat string. All byte<->line
    // conversions go through the rope (O(log n)) — never via linear char-index
    // scans, which were O(n²) over the whole file and made backward search on
    // multi-MB files hang for many seconds.
    let rope = buf.rope();
    let text = rope.to_string();

    let cursor_byte = {
        let row = origin.row.min(rope.len_lines().saturating_sub(1));
        let line = buf.line_string(row);
        let byte_in_line = twidth::col_to_byte(&line, origin.col, tw);
        (rope.line_to_byte(row) + byte_in_line).min(text.len())
    };

    let target_byte: usize = if forward {
        let start = if inclusive {
            cursor_byte
        } else {
            // Advance one byte past current cursor so n doesn't stick.
            cursor_byte + text[cursor_byte..].chars().next().map_or(0, char::len_utf8)
        };
        re.find(&text[start..])
            .map(|m| start + m.start())
            .or_else(|| re.find(&text).map(|m| m.start()))?
    } else {
        // Backward: the last match starting before the cursor; wrap if none.
        let before = &text[..cursor_byte];
        re.find_iter(before)
            .last()
            .map(|m| m.start())
            .or_else(|| re.find_iter(&text).last().map(|m| m.start()))?
    };

    // Convert byte offset back to (row, col) via the rope (O(log n)).
    let row = rope.byte_to_line(target_byte);
    let col_byte_in_line = target_byte - rope.line_to_byte(row);
    let line = buf.line_string(row);
    let col = twidth::byte_to_col(&line, col_byte_in_line, tw);
    Some((row, col))
}

