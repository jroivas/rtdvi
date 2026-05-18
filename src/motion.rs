//! Cursor motions registered as named actions.
//!
//! Each motion is a free function taking `&mut Editor`. They're wired into
//! the [`ActionRegistry`] in [`register_all`] and bound to default keys in
//! [`bind_default_keys`]. A future plugin layer or TOML config can rebind
//! any motion by name without touching this file.

use std::sync::Arc;

use crate::cursor::Cursor;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

/// Sentinel meaning "keep cursor at end-of-line on vertical motion".
pub const STICKY_EOL: usize = usize::MAX;

pub fn register_all(reg: &mut ActionRegistry) {
    // Motions that simply repeat per count.
    reg.register("move_left", Arc::new(|ed| with_window_repeat(ed, move_left)));
    reg.register("move_right", Arc::new(|ed| with_window_repeat(ed, move_right)));
    reg.register("move_up", Arc::new(|ed| with_window_repeat(ed, move_up)));
    reg.register("move_down", Arc::new(|ed| with_window_repeat(ed, move_down)));
    reg.register("word_forward", Arc::new(|ed| with_window_repeat(ed, word_forward)));
    reg.register("word_backward", Arc::new(|ed| with_window_repeat(ed, word_backward)));
    reg.register("word_end", Arc::new(|ed| with_window_repeat(ed, word_end)));
    // Motions where the count is absolute (line numbers).
    reg.register("line_start", Arc::new(|ed| { let _ = ed.take_count(); with_window_mut(ed, line_start); }));
    reg.register("line_end", Arc::new(|ed| { let _ = ed.take_count(); with_window_mut(ed, line_end); }));
    reg.register("first_line", Arc::new(goto_first_line));
    reg.register("last_line", Arc::new(goto_last_line));
    reg.register("page_down", Arc::new(page_down));
    reg.register("page_up", Arc::new(page_up));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    use ModeId::Normal;
    let bindings = [
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
        ("<C-f>", "page_down"),
        ("<C-u>", "page_up"),
        ("<PageDown>", "page_down"),
        ("<PageUp>", "page_up"),
    ];
    for (seq, action) in bindings {
        reg.bind(Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- Helpers ---------------------------------------------------------------

/// Run `f` against the active window's cursor `count` times.
fn with_window_repeat<F: Fn(&Editor, &mut Cursor)>(editor: &mut Editor, f: F) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let mut cursor = match editor.windows.get(&win_id) {
        Some(w) => w.cursor,
        None => return,
    };
    let tab_width = editor.config.options.tab_width;
    for _ in 0..count {
        f(editor, &mut cursor);
        let buf_id = match editor.windows.get(&win_id) {
            Some(w) => w.buffer,
            None => return,
        };
        if let Some(b) = editor.buffers.get(&buf_id) {
            clamp_to_buffer(&mut cursor, b, tab_width);
        }
    }
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor = cursor;
    }
    crate::event::emit(
        editor,
        crate::event::Event::CursorMoved { window: win_id },
    );
}

/// Move to line `count` (1-indexed). Default 1.
fn goto_first_line(editor: &mut Editor) {
    let n = editor
        .pending_count_pre
        .take()
        .or_else(|| editor.pending_count_post.take())
        .unwrap_or(1)
        .max(1);
    editor.jumplist_record_here();
    move_to_row(editor, n.saturating_sub(1));
}

/// `G`: jump to line `count` if given, else last line.
fn goto_last_line(editor: &mut Editor) {
    let count = editor
        .pending_count_pre
        .take()
        .or_else(|| editor.pending_count_post.take());
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let target_row = match count {
        Some(n) => n.saturating_sub(1),
        None => {
            // Ensure the full line index is built before jumping to the last
            // line — for large mmap files this triggers the remaining scan.
            if let Some(b) = editor.buffers.get_mut(&buf_id) {
                b.ensure_fully_indexed();
            }
            editor
                .buffers
                .get(&buf_id)
                .map(|b| b.line_count().saturating_sub(1))
                .unwrap_or(0)
        }
    };
    editor.jumplist_record_here();
    move_to_row(editor, target_row);
}

/// `<C-f>` — scroll forward one screen and land the cursor at the new top.
/// Leaves two lines of overlap with the previous screen (vim's behaviour).
fn page_down(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let viewport_h = editor
        .windows
        .get(&win_id)
        .map(|w| w.viewport_h as usize)
        .unwrap_or(24);
    let step = viewport_h.saturating_sub(2).max(1) * count;
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let last_row = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0);
    if let Some(w) = editor.windows.get_mut(&win_id) {
        let target = (w.cursor.row + step).min(last_row);
        w.cursor.row = target;
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
        // Snap viewport so the cursor sits near the top of the new screen.
        w.top_line = target.saturating_sub(1);
    }
    crate::event::emit(
        editor,
        crate::event::Event::CursorMoved { window: win_id },
    );
}

/// `<C-u>` — scroll back one screen and land the cursor at the new top.
fn page_up(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let viewport_h = editor
        .windows
        .get(&win_id)
        .map(|w| w.viewport_h as usize)
        .unwrap_or(24);
    let step = viewport_h.saturating_sub(2).max(1) * count;
    if let Some(w) = editor.windows.get_mut(&win_id) {
        let target = w.cursor.row.saturating_sub(step);
        w.cursor.row = target;
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
        w.top_line = target;
    }
    crate::event::emit(
        editor,
        crate::event::Event::CursorMoved { window: win_id },
    );
}

fn move_to_row(editor: &mut Editor, row: usize) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let buf_id = editor.windows.get(&win_id).unwrap().buffer;
    let last = editor
        .buffers
        .get(&buf_id)
        .map(|b| b.line_count().saturating_sub(1))
        .unwrap_or(0);
    let row = row.min(last);
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
    crate::event::emit(
        editor,
        crate::event::Event::CursorMoved { window: win_id },
    );
}

/// Run `f` against the active window with the active buffer context.
/// Centralizes the "no active window" guard.
fn with_window_mut<F: FnOnce(&Editor, &mut Cursor)>(editor: &mut Editor, f: F) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    // We need both `&Editor` (for buffer / config) and `&mut Cursor`.
    // Take the window cursor out, mutate, put back. Simpler than splitting borrows.
    let mut cursor = match editor.windows.get(&win_id) {
        Some(w) => w.cursor,
        None => return,
    };
    f(editor, &mut cursor);
    let buf_id = editor.windows.get(&win_id).map(|w| w.buffer);
    if let Some(buf_id) = buf_id {
        if let Some(b) = editor.buffers.get(&buf_id) {
            clamp_to_buffer(&mut cursor, b, editor.config.options.tab_width);
        }
    }
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor = cursor;
    }
    crate::event::emit(
        editor,
        crate::event::Event::CursorMoved { window: win_id },
    );
}

fn clamp_to_buffer(cursor: &mut Cursor, buffer: &crate::buffer::Buffer, tab_width: usize) {
    let last_row = buffer.line_count().saturating_sub(1);
    if cursor.row > last_row {
        cursor.row = last_row;
    }
    let line = buffer.line_string(cursor.row);
    let max_col = twidth::line_display_width(&line, tab_width);
    // Normal mode: cursor sits *on* a character, so max valid col is max_col-1
    // for non-empty lines, 0 for empty. Don't update sticky_col here unless
    // the move sets it explicitly.
    let cap = max_col.saturating_sub(1);
    if cursor.col == STICKY_EOL {
        cursor.col = cap;
    } else if cursor.col > cap {
        cursor.col = cap;
    }
}

fn line_width(buffer: &crate::buffer::Buffer, row: usize, tab_width: usize) -> usize {
    twidth::line_display_width(&buffer.line_string(row), tab_width)
}

// ---- Motions ---------------------------------------------------------------

/// `h` — step left by one **grapheme**. Tab characters occupy several
/// display cells but count as one grapheme, so `h` jumps over the whole
/// tab in a single press. If the cursor is mid-grapheme (e.g. landed
/// inside a tab via sticky-col), this snaps to the start of that
/// grapheme rather than stepping one cell left.
fn move_left(ed: &Editor, c: &mut Cursor) {
    if c.col == 0 {
        c.sticky_col = 0;
        return;
    }
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let line = buf.line_string(c.row);
    // Walk graphemes; remember the last start_col strictly less than c.col.
    let mut prev_start = 0usize;
    for (_b, _g, sc, _w) in twidth::graphemes_with_cols(&line, tw) {
        if sc >= c.col {
            break;
        }
        prev_start = sc;
    }
    c.col = prev_start;
    c.sticky_col = c.col;
}

/// `l` — step right by one **grapheme**, jumping over multi-cell ones
/// (tabs, CJK) in one move. Mid-grapheme cursors snap forward to the
/// start of the next grapheme.
fn move_right(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let line = buf.line_string(c.row);
    let cap = twidth::line_display_width(&line, tw).saturating_sub(1);
    if c.col >= cap {
        c.sticky_col = c.col;
        return;
    }
    // Find the grapheme that contains c.col, then land on the next.
    let mut landed = false;
    for (_b, _g, sc, w) in twidth::graphemes_with_cols(&line, tw) {
        if sc <= c.col && c.col < sc + w {
            c.col = (sc + w).min(cap);
            landed = true;
            break;
        }
    }
    if !landed {
        // Cursor was past every grapheme (shouldn't normally happen given
        // cap check above) — leave it alone.
    }
    c.sticky_col = c.col;
}

fn move_up(ed: &Editor, c: &mut Cursor) {
    if c.row == 0 {
        return;
    }
    c.row -= 1;
    apply_sticky(ed, c);
}

fn move_down(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    if c.row + 1 >= buf.line_count() {
        return;
    }
    c.row += 1;
    apply_sticky(ed, c);
}

fn apply_sticky(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let w = line_width(buf, c.row, tw);
    let cap = w.saturating_sub(1);
    c.col = if c.sticky_col == STICKY_EOL { cap } else { c.sticky_col.min(cap) };
}

fn line_start(_ed: &Editor, c: &mut Cursor) {
    c.col = 0;
    c.sticky_col = 0;
}

fn line_end(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let w = line_width(buf, c.row, ed.config.options.tab_width);
    c.col = w.saturating_sub(1);
    c.sticky_col = STICKY_EOL;
}

// `first_line` / `last_line` live as `goto_first_line` / `goto_last_line`
// above — they handle vim-style count semantics directly.

// ---- Word motions ----------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
enum CharClass {
    Blank,
    Keyword,
    Punct,
}

fn classify(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Keyword
    } else {
        CharClass::Punct
    }
}

/// Linear (row, col) walker over the buffer treating display columns. Word
/// motions step grapheme-by-grapheme rather than display-cell-by-cell so
/// CJK words still advance one character at a time.
fn word_forward(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let (mut row, mut byte) = (c.row, twidth::col_to_byte(&buf.line_string(c.row), c.col, tw));
    let start_class = char_class_at(buf, row, byte);
    // Phase 1: walk through the rest of the starting class (skip current word).
    if start_class != CharClass::Blank {
        while let Some((r, b)) = step_forward(buf, row, byte) {
            row = r;
            byte = b;
            if char_class_at(buf, row, byte) != start_class {
                break;
            }
        }
    }
    // Phase 2: skip blanks until the next non-blank (next word start).
    while char_class_at(buf, row, byte) == CharClass::Blank {
        match step_forward(buf, row, byte) {
            Some((r, b)) => {
                row = r;
                byte = b;
            }
            None => break,
        }
    }
    place_cursor(ed, c, row, byte);
}

fn word_backward(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let (row, byte) = (c.row, twidth::col_to_byte(&buf.line_string(c.row), c.col, tw));
    // Step back once to look at the previous char; nothing to do if at start.
    let Some((mut r, mut b)) = step_backward(buf, row, byte) else {
        return;
    };
    // Skip blanks.
    while char_class_at(buf, r, b) == CharClass::Blank {
        match step_backward(buf, r, b) {
            Some((nr, nb)) => {
                r = nr;
                b = nb;
            }
            None => {
                place_cursor(ed, c, r, b);
                return;
            }
        }
    }
    let cls = char_class_at(buf, r, b);
    // Walk back through the same class.
    loop {
        let prev = match step_backward(buf, r, b) {
            Some(p) => p,
            None => break,
        };
        if char_class_at(buf, prev.0, prev.1) == cls {
            (r, b) = prev;
        } else {
            break;
        }
    }
    place_cursor(ed, c, r, b);
}

fn word_end(ed: &Editor, c: &mut Cursor) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let (mut row, mut byte) = (c.row, twidth::col_to_byte(&buf.line_string(c.row), c.col, tw));
    // Step forward at least once; skip blanks.
    if let Some((r, b)) = step_forward(buf, row, byte) {
        row = r;
        byte = b;
    } else {
        return;
    }
    while char_class_at(buf, row, byte) == CharClass::Blank {
        match step_forward(buf, row, byte) {
            Some((r, b)) => {
                row = r;
                byte = b;
            }
            None => {
                place_cursor(ed, c, row, byte);
                return;
            }
        }
    }
    let cls = char_class_at(buf, row, byte);
    // Walk forward until next class change, then back one.
    let (mut r, mut b) = (row, byte);
    loop {
        let nxt = match step_forward(buf, r, b) {
            Some(p) => p,
            None => break,
        };
        if char_class_at(buf, nxt.0, nxt.1) == cls {
            (r, b) = nxt;
        } else {
            break;
        }
    }
    row = r;
    byte = b;
    place_cursor(ed, c, row, byte);
}

// ---- Grapheme stepping helpers --------------------------------------------

fn active_buffer(ed: &Editor) -> Option<&crate::buffer::Buffer> {
    let win = ed.active_window()?;
    ed.buffers.get(&win.buffer)
}

/// Step one grapheme forward; wraps to next line. Stops at end of buffer.
fn step_forward(buf: &crate::buffer::Buffer, row: usize, byte: usize) -> Option<(usize, usize)> {
    let line = buf.line_string(row);
    if byte < line.len() {
        // Next grapheme boundary.
        let next = line[byte..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| byte + i)
            .unwrap_or(line.len());
        Some((row, next))
    } else if row + 1 < buf.line_count() {
        Some((row + 1, 0))
    } else {
        None
    }
}

fn step_backward(buf: &crate::buffer::Buffer, row: usize, byte: usize) -> Option<(usize, usize)> {
    if byte > 0 {
        let line = buf.line_string(row);
        let prev = line[..byte]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        Some((row, prev))
    } else if row > 0 {
        let new_row = row - 1;
        let line = buf.line_string(new_row);
        Some((new_row, line.len().saturating_sub(1).min(line.len())))
    } else {
        None
    }
}

fn char_class_at(buf: &crate::buffer::Buffer, row: usize, byte: usize) -> CharClass {
    let line = buf.line_string(row);
    line[byte..]
        .chars()
        .next()
        .map(classify)
        .unwrap_or(CharClass::Blank)
}

fn place_cursor(ed: &Editor, c: &mut Cursor, row: usize, byte: usize) {
    let buf = match active_buffer(ed) {
        Some(b) => b,
        None => return,
    };
    let tw = ed.config.options.tab_width;
    let line = buf.line_string(row);
    c.row = row;
    c.col = twidth::byte_to_col(&line, byte, tw);
    c.sticky_col = c.col;
}
