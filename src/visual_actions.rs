//! Actions specific to visual modes: enter visual, delete/yank/change.

use std::sync::Arc;

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::{switch_mode, ModeId};
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("enter_visual", Arc::new(enter_visual));
    reg.register("enter_visual_line", Arc::new(enter_visual_line));
    reg.register("enter_visual_block", Arc::new(enter_visual_block));
    reg.register("visual_delete", Arc::new(visual_delete));
    reg.register("visual_yank", Arc::new(visual_yank));
    reg.register("visual_change", Arc::new(visual_change));
    reg.register("paste_after", Arc::new(paste_after));
    reg.register("paste_before", Arc::new(paste_before));
    reg.register("block_insert_at_left", Arc::new(block_insert_at_left));
    reg.register("block_append_at_right", Arc::new(block_append_at_right));
}

/// Bind enter-visual keys + paste under Normal; mirror motion + d/y/c under
/// the visual modes themselves.
pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    // Normal -> visual entry + paste.
    let normal_bindings = [
        ("v", "enter_visual"),
        ("V", "enter_visual_line"),
        ("<C-v>", "enter_visual_block"),
        ("p", "paste_after"),
        ("P", "paste_before"),
    ];
    for (seq, action) in normal_bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
    // Motions under visual modes (same names as in normal).
    let motions = [
        ("h", "move_left"),
        ("l", "move_right"),
        ("k", "move_up"),
        ("j", "move_down"),
        ("0", "line_start"),
        ("$", "line_end"),
        ("gg", "first_line"),
        ("G", "last_line"),
        ("w", "word_forward"),
        ("b", "word_backward"),
        ("e", "word_end"),
        ("<Left>", "move_left"),
        ("<Right>", "move_right"),
        ("<Up>", "move_up"),
        ("<Down>", "move_down"),
    ];
    for mode in [ModeId::Visual, ModeId::VisualLine, ModeId::VisualBlock] {
        for (seq, action) in motions {
            reg.bind(mode, seq, Action::Builtin(action)).unwrap();
        }
        // d / y / c.
        reg.bind(mode, "d", Action::Builtin("visual_delete")).unwrap();
        reg.bind(mode, "y", Action::Builtin("visual_yank")).unwrap();
        reg.bind(mode, "c", Action::Builtin("visual_change")).unwrap();
        reg.bind(mode, "x", Action::Builtin("visual_delete")).unwrap();
    }
    // Visual-block extras.
    reg.bind(ModeId::VisualBlock, "I", Action::Builtin("block_insert_at_left")).unwrap();
    reg.bind(ModeId::VisualBlock, "A", Action::Builtin("block_append_at_right")).unwrap();
}

fn enter_visual(editor: &mut Editor) {
    let Some(w) = editor.active_window_mut() else {
        return;
    };
    w.selection = Selection::Char { anchor: w.cursor };
    switch_mode(editor, ModeId::Visual);
}

fn enter_visual_line(editor: &mut Editor) {
    let Some(w) = editor.active_window_mut() else {
        return;
    };
    w.selection = Selection::Line { anchor_row: w.cursor.row };
    switch_mode(editor, ModeId::VisualLine);
}

fn enter_visual_block(editor: &mut Editor) {
    let Some(w) = editor.active_window_mut() else {
        return;
    };
    w.selection = Selection::Block { anchor: w.cursor };
    switch_mode(editor, ModeId::VisualBlock);
}

/// Compute the (start_char, end_char) range from a character-wise selection
/// in the active window, plus a flag indicating linewise selection.
pub(crate) fn selection_char_range(editor: &Editor) -> Option<(usize, usize, bool)> {
    let win = editor.active_window()?;
    let buf = editor.buffers.get(&win.buffer)?;
    let tw = editor.config.options.tab_width;
    match win.selection {
        Selection::None => None,
        Selection::Char { anchor } => Some((
            char_index(buf, normalize_pair(anchor, win.cursor).0, tw),
            // End is *exclusive* — bump one char past the head's grapheme.
            {
                let (_, tail) = normalize_pair(anchor, win.cursor);
                let after = step_one_char(buf, tail, tw);
                char_index(buf, after, tw)
            },
            false,
        )),
        Selection::Line { anchor_row } => {
            let cur_row = win.cursor.row;
            let (top, bot) = if anchor_row <= cur_row {
                (anchor_row, cur_row)
            } else {
                (cur_row, anchor_row)
            };
            let start = buf.line_to_char(top);
            // End = start of (bot + 1), capped to buffer end.
            let end = if bot + 1 >= buf.line_count() {
                buf.len_chars()
            } else {
                buf.line_to_char(bot + 1)
            };
            Some((start, end, true))
        }
        Selection::Block { .. } => None,
    }
}

/// `Cursor` representing the smaller of two cursors in document order.
fn normalize_pair(a: Cursor, b: Cursor) -> (Cursor, Cursor) {
    if (a.row, a.col) <= (b.row, b.col) {
        (a, b)
    } else {
        (b, a)
    }
}

fn char_index(buf: &Buffer, cursor: Cursor, tw: usize) -> usize {
    let line_start = buf.line_to_char(cursor.row);
    let line = buf.line_string(cursor.row);
    let byte = twidth::col_to_byte(&line, cursor.col, tw);
    line_start + line[..byte].chars().count()
}

/// Return a cursor positioned one grapheme past `c` (clamped to end of line).
fn step_one_char(buf: &Buffer, c: Cursor, tw: usize) -> Cursor {
    let line = buf.line_string(c.row);
    let byte = twidth::col_to_byte(&line, c.col, tw);
    if byte >= line.len() {
        return c;
    }
    let next_byte = line[byte..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| byte + i)
        .unwrap_or(line.len());
    let next_col = twidth::byte_to_col(&line, next_byte, tw);
    Cursor { row: c.row, col: next_col, sticky_col: next_col }
}

fn visual_delete(editor: &mut Editor) {
    if matches!(
        editor.active_window().map(|w| w.selection),
        Some(Selection::Block { .. })
    ) {
        block_delete(editor);
        return;
    }
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let edit = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        b.delete(start..end)
    };
    crate::registers::store(editor, edit.removed.clone(), linewise);
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &edit },
    );
    // Move cursor to the start of the deletion.
    if let Some(b) = editor.buffers.get(&buf_id) {
        let row = b.char_to_line(start);
        let line = b.line_string(row);
        let line_start = b.line_to_char(row);
        let off_chars = start.saturating_sub(line_start);
        // Find byte offset for that char offset in `line`.
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_chars {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, editor.config.options.tab_width);
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = row;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
            w.selection = Selection::None;
        }
    }
    switch_mode(editor, ModeId::Normal);
}

fn visual_yank(editor: &mut Editor) {
    if matches!(
        editor.active_window().map(|w| w.selection),
        Some(Selection::Block { .. })
    ) {
        block_yank(editor);
        return;
    }
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let text: String = match editor.buffers.get(&buf_id) {
        Some(b) => b.rope().slice(start..end).to_string(),
        None => return,
    };
    crate::registers::store(editor, text, linewise);
    switch_to_normal_clear(editor);
}

fn visual_change(editor: &mut Editor) {
    if matches!(
        editor.active_window().map(|w| w.selection),
        Some(Selection::Block { .. })
    ) {
        // Open one transaction covering the block delete AND the subsequent
        // insert session; the Esc handler will close it.
        editor.begin_active_transaction();
        block_delete(editor);
        switch_mode(editor, ModeId::Insert);
        return;
    }
    // Non-block change: open a transaction so the delete + typed insert
    // collapse into one undo step.
    editor.begin_active_transaction();
    let Some((start, end, linewise)) = selection_char_range(editor) else {
        return;
    };
    if start >= end {
        switch_to_normal_clear(editor);
        return;
    }
    let buf_id = editor.active_buffer_id().unwrap();
    let removed = {
        let b = editor.buffers.get_mut(&buf_id).unwrap();
        let edit = b.delete(start..end);
        edit.removed
    };
    crate::registers::store(editor, removed, linewise);
    if let Some(b) = editor.buffers.get(&buf_id) {
        let row = b.char_to_line(start);
        let line_start = b.line_to_char(row);
        let off_chars = start.saturating_sub(line_start);
        let line = b.line_string(row);
        let mut byte = line.len();
        for (i, (b_off, c)) in line.char_indices().enumerate() {
            if i == off_chars {
                byte = b_off;
                break;
            }
            byte = b_off + c.len_utf8();
        }
        let col = twidth::byte_to_col(&line, byte, editor.config.options.tab_width);
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = row;
            w.cursor.col = col;
            w.cursor.sticky_col = col;
            w.selection = Selection::None;
        }
    }
    switch_mode(editor, ModeId::Insert);
}

/// `p` — paste after the cursor (charwise) / below the line (linewise).
fn paste_after(editor: &mut Editor) {
    paste_register(editor, false);
}

/// `P` — paste before the cursor (charwise) / above the line (linewise).
fn paste_before(editor: &mut Editor) {
    paste_register(editor, true);
}

/// Shared paste body. `before` selects vim's `P` placement (at the cursor /
/// above the current line) instead of `p`'s (after the cursor / below). The
/// cursor-advance below is identical either way — it always starts from the
/// original cursor column, which is where the inserted text begins for `P`
/// and one cell on for `p`.
fn paste_register(editor: &mut Editor, before: bool) {
    let reg = crate::registers::read_for_paste(editor);
    if reg.text.is_empty() {
        return;
    }
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let (buf_id, cursor) = match editor.windows.get(&win_id) {
        Some(w) => (w.buffer, w.cursor),
        None => return,
    };
    let tw = editor.config.options.tab_width;
    if reg.linewise {
        // Insert at the start of the current line (`P`) or the next line (`p`).
        let row = if before { cursor.row } else { cursor.row + 1 };
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        let insert_at = if row >= buf.line_count() {
            // Append at end-of-buffer; ensure leading newline.
            let end = buf.len_chars();
            // If buffer doesn't end with newline, add one first.
            let needs_nl = end == 0
                || buf.rope().slice(end.saturating_sub(1)..end).to_string() != "\n";
            if needs_nl {
                buf.insert(end, "\n");
            }
            buf.len_chars()
        } else {
            buf.line_to_char(row)
        };
        // Ensure trailing newline on inserted text so the next line stays intact.
        let mut text = reg.text.clone();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let _ = buf.insert(insert_at, &text);
        // Move cursor to first non-blank of the new line (approximation: col 0).
        if let Some(w) = editor.windows.get_mut(&win_id) {
            w.cursor.row = row.min(buf.line_count().saturating_sub(1));
            w.cursor.col = 0;
            w.cursor.sticky_col = 0;
        }
    } else {
        // Insert at the cursor's char (`P`) or after it (`p`).
        let buf = editor.buffers.get(&buf_id).unwrap();
        let at = if before { cursor } else { step_one_char(buf, cursor, tw) };
        let char_idx = char_index(buf, at, tw);
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        let _ = buf.insert(char_idx, &reg.text);
        // Place cursor on the last inserted char.
        // (Approximation: bump the cursor by reg.text grapheme width on same line.)
        if let Some(w) = editor.windows.get_mut(&win_id) {
            for ch in reg.text.chars() {
                if ch == '\n' {
                    w.cursor.row += 1;
                    w.cursor.col = 0;
                } else {
                    let g = ch.to_string();
                    let wid = twidth::grapheme_width(&g, w.cursor.col, tw).max(1);
                    w.cursor.col += wid;
                }
            }
            // Step back one to land on the last inserted cell.
            if w.cursor.col > 0 {
                w.cursor.col -= 1;
            }
            w.cursor.sticky_col = w.cursor.col;
        }
    }
    let synthetic = crate::buffer::Edit { range: 0..0, removed: String::new(), inserted: String::new() };
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &synthetic },
    );
}

fn switch_to_normal_clear(editor: &mut Editor) {
    if let Some(w) = editor.active_window_mut() {
        w.selection = Selection::None;
    }
    switch_mode(editor, ModeId::Normal);
}

// ---- Block (visual-block) helpers -----------------------------------------

/// Resolve the active rectangle as `(top_row, bot_row, left_col, right_col)`,
/// each inclusive. `None` if the selection isn't a block.
fn block_rect(editor: &Editor) -> Option<(usize, usize, usize, usize)> {
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

/// For each row in the rectangle, compute `(char_start, char_end, slice_text)`
/// — char indices into the buffer and the displayed text in that slice.
fn rect_row_ranges(editor: &Editor) -> Vec<(usize, usize, String)> {
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

fn block_delete(editor: &mut Editor) {
    let rows = rect_row_ranges(editor);
    if rows.is_empty() {
        switch_to_normal_clear(editor);
        return;
    }
    let yanked = rows
        .iter()
        .map(|(_, _, t)| t.clone())
        .collect::<Vec<_>>()
        .join("\n");
    crate::registers::store(editor, yanked, false);
    let buf_id = editor.active_buffer_id().unwrap();
    // Coalesce all per-row deletes into one undo step. If a caller has
    // already opened a transaction (block-change does), nest cleanly: the
    // outer `begin` would commit ours first, so we just append.
    let opened_here = !editor.buffers.get(&buf_id).map(|b| b.in_transaction()).unwrap_or(false);
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().begin_transaction();
    }
    {
        let buf = editor.buffers.get_mut(&buf_id).unwrap();
        for (start, end, _) in rows.iter().rev() {
            if end > start {
                let _ = buf.delete(*start..*end);
            }
        }
    }
    if opened_here {
        editor.buffers.get_mut(&buf_id).unwrap().end_transaction();
    }
    let synthetic = crate::buffer::Edit { range: 0..0, removed: String::new(), inserted: String::new() };
    crate::event::emit(
        editor,
        crate::event::Event::BufferChanged { buffer: buf_id, edit: &synthetic },
    );
    let (top, _, left, _) = block_rect(editor).unwrap_or((0, 0, 0, 0));
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = top;
        w.cursor.col = left;
        w.cursor.sticky_col = left;
        w.selection = Selection::None;
    }
    switch_mode(editor, ModeId::Normal);
}

fn block_yank(editor: &mut Editor) {
    let rows = rect_row_ranges(editor);
    let yanked = rows
        .iter()
        .map(|(_, _, t)| t.clone())
        .collect::<Vec<_>>()
        .join("\n");
    crate::registers::store(editor, yanked, false);
    // Cursor returns to top-left of the rectangle.
    if let Some((top, _, left, _)) = block_rect(editor) {
        if let Some(w) = editor.active_window_mut() {
            w.cursor.row = top;
            w.cursor.col = left;
            w.cursor.sticky_col = left;
        }
    }
    switch_to_normal_clear(editor);
}

/// `I` in visual-block: enter insert mode at the left edge of the top row.
/// On `<Esc>` from insert mode, the inserted text is replayed into every
/// other selected row by [`crate::mode::insert`].
fn block_insert_at_left(editor: &mut Editor) {
    let Some((top, bot, left, _right)) = block_rect(editor) else {
        return;
    };
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = top;
        w.cursor.col = left;
        w.cursor.sticky_col = left;
        w.selection = Selection::None;
    }
    editor.pending_block_insert = Some(crate::editor::PendingBlockInsert {
        other_rows: ((top + 1)..=bot).collect(),
        col: left,
        start_row: top,
        start_col: left,
        pad_when_short: false,
    });
    // Open one transaction that covers both the typing on the top row and
    // the cross-row replay on `<Esc>` — single undo step.
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

fn block_append_at_right(editor: &mut Editor) {
    let Some((top, bot, _left, right)) = block_rect(editor) else {
        return;
    };
    let insert_col = right + 1;
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = top;
        w.cursor.col = insert_col;
        w.cursor.sticky_col = w.cursor.col;
        w.selection = Selection::None;
    }
    editor.pending_block_insert = Some(crate::editor::PendingBlockInsert {
        other_rows: ((top + 1)..=bot).collect(),
        col: insert_col,
        start_row: top,
        start_col: insert_col,
        pad_when_short: true,
    });
    editor.begin_active_transaction();
    switch_mode(editor, ModeId::Insert);
}

/// Apply a queued block-insert replay: copy the text typed on `start_row`
/// (from `start_col` to the current cursor) into every other row of the
/// rectangle at `col`. Called by the insert-mode `<Esc>` handler.
pub fn apply_block_insert_replay(
    editor: &mut Editor,
    pending: crate::editor::PendingBlockInsert,
) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some((buf_id, cursor)) = editor.windows.get(&win_id).map(|w| (w.buffer, w.cursor)) else {
        return;
    };
    let tw = editor.config.options.tab_width;

    // Capture the text that was typed on the start row.
    let typed: String = {
        let Some(buf) = editor.buffers.get(&buf_id) else {
            return;
        };
        let line = buf.line_string(pending.start_row);
        let start_byte = twidth::col_to_byte(&line, pending.start_col, tw);
        let end_byte = if cursor.row == pending.start_row {
            twidth::col_to_byte(&line, cursor.col, tw)
        } else {
            line.len()
        };
        if end_byte <= start_byte {
            String::new()
        } else {
            line[start_byte..end_byte].to_string()
        }
    };
    if typed.is_empty() {
        return;
    }

    for row in &pending.other_rows {
        let row = *row;
        let action: Option<(usize, String)> = {
            let Some(buf) = editor.buffers.get(&buf_id) else {
                return;
            };
            if row >= buf.line_count() {
                None
            } else {
                let line = buf.line_string(row);
                let line_width = twidth::line_display_width(&line, tw);
                let line_start_char = buf.line_to_char(row);
                if line_width < pending.col {
                    if !pending.pad_when_short {
                        None
                    } else {
                        let pad: String = std::iter::repeat(' ')
                            .take(pending.col - line_width)
                            .collect();
                        let insert_at = line_start_char + line.chars().count();
                        Some((insert_at, format!("{pad}{typed}")))
                    }
                } else {
                    let byte = twidth::col_to_byte(&line, pending.col, tw);
                    let char_off = line[..byte].chars().count();
                    Some((line_start_char + char_off, typed.clone()))
                }
            }
        };
        if let Some((char_idx, text)) = action {
            if let Some(buf) = editor.buffers.get_mut(&buf_id) {
                let _ = buf.insert(char_idx, &text);
            }
        }
    }

    // Land the cursor at the start of the replay (vim leaves it at the
    // top-left of the block-insert).
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = pending.start_row;
        w.cursor.col = pending.start_col;
        w.cursor.sticky_col = pending.start_col;
    }
}
