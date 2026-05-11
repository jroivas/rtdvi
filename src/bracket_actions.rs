//! Bracket and section motions.
//!
//! * `%` jumps between matching `()`, `[]`, `{}`. If the cursor isn't on a
//!   bracket, vim scans forward on the current line for the first bracket
//!   character and starts from there.
//! * `[[` / `]]` jump to the previous / next line that begins with `{` —
//!   vim's convention for "section" / "function" boundaries in C-like code.
//!
//! All three are count-aware: `3%` doesn't really make sense (vim treats
//! it as a line-percentage instead), but `2]]` jumps two sections forward.

use std::sync::Arc;

use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("match_bracket", Arc::new(match_bracket));
    reg.register("section_forward", Arc::new(section_forward));
    reg.register("section_backward", Arc::new(section_backward));
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    // Available in Normal AND every visual mode — they're motions.
    for mode in [
        ModeId::Normal,
        ModeId::Visual,
        ModeId::VisualLine,
        ModeId::VisualBlock,
    ] {
        reg.bind(mode, "%", Action::Builtin("match_bracket")).unwrap();
        reg.bind(mode, "]]", Action::Builtin("section_forward")).unwrap();
        reg.bind(mode, "[[", Action::Builtin("section_backward")).unwrap();
    }
}

// ---- Cursor <-> char helpers ----------------------------------------------

fn cursor_to_char(buf: &Buffer, c: Cursor, tw: usize) -> usize {
    let line_start = buf.line_to_char(c.row);
    let line = buf.line_string(c.row);
    let byte = twidth::col_to_byte(&line, c.col, tw);
    line_start + line[..byte].chars().count()
}

fn place_cursor_at_char(editor: &mut Editor, char_idx: usize) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    let Some(buf_id) = editor.windows.get(&win_id).map(|w| w.buffer) else {
        return;
    };
    let Some(b) = editor.buffers.get(&buf_id) else {
        return;
    };
    let tw = editor.config.options.tab_width;
    let total = b.len_chars();
    let idx = char_idx.min(total);
    let row = b.char_to_line(idx);
    let line_start = b.line_to_char(row);
    let off_chars = idx.saturating_sub(line_start);
    let line = b.line_string(row);
    let mut byte = line.len();
    for (i, (b_off, c)) in line.char_indices().enumerate() {
        if i == off_chars {
            byte = b_off;
            break;
        }
        byte = b_off + c.len_utf8();
    }
    let col = twidth::byte_to_col(&line, byte, tw);
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = col;
        w.cursor.sticky_col = col;
    }
}

fn move_to_row(editor: &mut Editor, row: usize) {
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = row;
        w.cursor.col = 0;
        w.cursor.sticky_col = 0;
    }
}

// ---- % match-bracket -------------------------------------------------------

fn match_bracket(editor: &mut Editor) {
    let _ = editor.take_count(); // `%` ignores count in v1
    let Some(win) = editor.active_window() else {
        return;
    };
    let buf_id = win.buffer;
    let cursor = win.cursor;
    let tw = editor.config.options.tab_width;
    let Some(buf) = editor.buffers.get(&buf_id) else {
        return;
    };

    // Starting char index. If that char isn't a bracket, vim's `%` searches
    // forward on the current line for the first bracket; we do the same.
    let start_char = cursor_to_char(buf, cursor, tw);
    let line_start = buf.line_to_char(cursor.row);
    let line = buf.line_string(cursor.row);
    let line_end_char = line_start + line.chars().count();

    let rope = buf.rope();
    let mut at = start_char;
    if at >= rope.len_chars() || !is_bracket(rope.char(at)) {
        // Scan forward on the current line.
        let mut scanned = None;
        let mut i = at.max(line_start);
        while i < line_end_char.min(rope.len_chars()) {
            if is_bracket(rope.char(i)) {
                scanned = Some(i);
                break;
            }
            i += 1;
        }
        match scanned {
            Some(i) => at = i,
            None => return,
        }
    }

    let Some(target) = find_match(rope, at) else {
        return;
    };
    place_cursor_at_char(editor, target);
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}')
}

fn find_match(rope: &ropey::Rope, pos: usize) -> Option<usize> {
    let total = rope.len_chars();
    if pos >= total {
        return None;
    }
    let here = rope.char(pos);
    let (open, close, forward) = match here {
        '(' => ('(', ')', true),
        ')' => ('(', ')', false),
        '[' => ('[', ']', true),
        ']' => ('[', ']', false),
        '{' => ('{', '}', true),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth: i32 = 1;
    if forward {
        let mut i = pos + 1;
        while i < total {
            let c = rope.char(i);
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            i += 1;
        }
    } else {
        let mut i = pos;
        while i > 0 {
            i -= 1;
            let c = rope.char(i);
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
    }
    None
}

// ---- Section motions -------------------------------------------------------

fn section_forward(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let Some(buf) = editor.buffers.get(&win.buffer) else {
        return;
    };
    let last = buf.line_count().saturating_sub(1);
    let mut row = win.cursor.row;
    for _ in 0..count {
        let mut found = None;
        let start = row + 1;
        for r in start..=last {
            if buf.line_string(r).starts_with('{') {
                found = Some(r);
                break;
            }
        }
        row = match found {
            Some(r) => r,
            None => {
                row = last;
                break;
            }
        };
    }
    move_to_row(editor, row);
}

fn section_backward(editor: &mut Editor) {
    let count = editor.take_count();
    let Some(win) = editor.active_window() else {
        return;
    };
    let Some(buf) = editor.buffers.get(&win.buffer) else {
        return;
    };
    let mut row = win.cursor.row;
    for _ in 0..count {
        if row == 0 {
            break;
        }
        let mut found = None;
        for r in (0..row).rev() {
            if buf.line_string(r).starts_with('{') {
                found = Some(r);
                break;
            }
        }
        row = match found {
            Some(r) => r,
            None => 0,
        };
    }
    move_to_row(editor, row);
}
