//! Shared helpers for the `*_actions.rs` operator families.
//!
//! These factor out logic that was previously copy-pasted across the
//! delete/yank/replace/visual modules: linewise range math, the
//! "run a motion and measure the span it covered" dance, and visual-block
//! rectangle geometry.

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::text::width as twidth;
use crate::Editor;

/// Char range `[lo, hi)` covering whole lines `[top_row, end_row_exclusive)`.
///
/// `hi` is clamped to the buffer's char length when `end_row_exclusive` runs
/// past the last line, so the final line's trailing newline (or its absence)
/// is handled the same way vim's linewise operators do. `top_row` /
/// `end_row_exclusive` may be passed unclamped — the clamp here absorbs it.
pub(crate) fn line_span_chars(buf: &Buffer, top_row: usize, end_row_exclusive: usize) -> (usize, usize) {
    let last_row = buf.line_count().saturating_sub(1);
    let lo = buf.line_to_char(top_row);
    let hi = if end_row_exclusive > last_row {
        buf.len_chars()
    } else {
        buf.line_to_char(end_row_exclusive)
    };
    (lo, hi)
}

/// Run `motion_name` from the active window's cursor with `count` armed, and
/// return `(lo, hi, start_cursor)` where `[lo, hi)` is the char span from the
/// original cursor to the motion's landing point. `inclusive` extends the span
/// one char past the target (e.g. `e` / `de` / `ye`).
///
/// The cursor is left wherever the motion put it; `start_cursor` is returned so
/// callers that must not move it (yank) can restore. Returns `None` if there is
/// no active window/buffer or `motion_name` is not a registered action.
pub(crate) fn motion_char_range(
    editor: &mut Editor,
    motion_name: &str,
    count: usize,
    inclusive: bool,
) -> Option<(usize, usize, Cursor)> {
    let win_id = editor.tabs.get(editor.active_tab).map(|t| t.active)?;
    let buf_id = editor.windows.get(&win_id).map(|w| w.buffer)?;
    let start = editor.windows.get(&win_id)?.cursor;

    // Re-arm the count so the motion action sees it.
    editor.pending_count_pre = Some(count);
    editor.pending_count_post = None;

    let action = editor.actions.lookup(motion_name)?;
    action(editor);

    let end = editor.windows.get(&win_id)?.cursor;
    let tw = editor.config.options.tab_width;
    let b = editor.buffers.get(&buf_id)?;
    let s = b.cursor_to_char(start, tw);
    let mut e = b.cursor_to_char(end, tw);
    if inclusive {
        e = e.saturating_add(1).min(b.len_chars());
    }
    let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
    Some((lo, hi, start))
}

/// The active visual-block rectangle as inclusive `(top, bot, left, right)`
/// rows/columns, or `None` when the selection isn't a block.
pub(crate) fn block_rect(editor: &Editor) -> Option<(usize, usize, usize, usize)> {
    let w = editor.active_window()?;
    let anchor = match w.selection {
        Selection::Block { anchor } => anchor,
        _ => return None,
    };
    let cur = w.cursor;
    Some((
        anchor.row.min(cur.row),
        anchor.row.max(cur.row),
        anchor.col.min(cur.col),
        anchor.col.max(cur.col),
    ))
}

/// For each existing row in the active visual-block rectangle, compute
/// `(char_start, char_end, slice_text)` — char indices into the buffer plus
/// the displayed text of that cell range. Rows past the buffer end are
/// skipped; rows shorter than the rectangle contribute an empty range
/// (`start == end`, empty text).
pub(crate) fn rect_row_ranges(editor: &Editor) -> Vec<(usize, usize, String)> {
    let Some((top, bot, left, right)) = block_rect(editor) else {
        return Vec::new();
    };
    let Some(w) = editor.active_window() else {
        return Vec::new();
    };
    let Some(buf) = editor.buffers.get(&w.buffer) else {
        return Vec::new();
    };
    let tw = editor.config.options.tab_width;
    let mut out = Vec::new();
    for row in top..=bot {
        if row >= buf.line_count() {
            break;
        }
        let line = buf.line_string(row);
        let left_byte = twidth::col_to_byte(&line, left, tw);
        let right_byte = twidth::col_to_byte(&line, right + 1, tw);
        let line_start = buf.line_to_char(row);
        let left_char_off = line[..left_byte].chars().count();
        let inner = &line[left_byte..right_byte];
        let inner_chars = inner.chars().count();
        out.push((
            line_start + left_char_off,
            line_start + left_char_off + inner_chars,
            inner.to_string(),
        ));
    }
    out
}
