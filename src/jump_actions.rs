//! Jumplist navigation (`<C-o>` / `<C-i>`) and "search word under cursor"
//! (`*`, `#`, `£`).

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register("jump_back", Arc::new(jump_back));
    reg.register("jump_forward", Arc::new(jump_forward));
    reg.register("search_word_under_cursor", Arc::new(search_word_under_cursor));
    reg.register(
        "search_word_under_cursor_backward",
        Arc::new(search_word_under_cursor_backward),
    );
}

pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    let bindings = [
        ("<C-o>", "jump_back"),
        // Terminals usually send <Tab> for Ctrl-I, so we bind that.
        ("<Tab>", "jump_forward"),
        ("*", "search_word_under_cursor"),
        ("£", "search_word_under_cursor"), // user's preferred binding
        ("#", "search_word_under_cursor_backward"),
    ];
    for (seq, action) in bindings {
        reg.bind(ModeId::Normal, seq, Action::Builtin(action)).unwrap();
    }
}

// ---- <C-o> / <C-i> --------------------------------------------------------

fn jump_back(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(current) = editor.current_jump_entry() else {
        return;
    };
    if let Some(entry) = editor.jumplist.back(current) {
        editor.jumplist_goto(entry);
    } else {
        editor.status_message = Some("Already at oldest jump".into());
    }
}

fn jump_forward(editor: &mut Editor) {
    let _ = editor.take_count();
    if let Some(entry) = editor.jumplist.forward() {
        editor.jumplist_goto(entry);
    } else {
        editor.status_message = Some("Already at newest jump".into());
    }
}

// ---- * / # / £ ------------------------------------------------------------

/// `*` — search forward for the word under the cursor.
fn search_word_under_cursor(editor: &mut Editor) {
    search_word(editor, true);
}

fn search_word_under_cursor_backward(editor: &mut Editor) {
    search_word(editor, false);
}

fn search_word(editor: &mut Editor, forward: bool) {
    let _ = editor.take_count();
    let Some(word) = word_under_cursor(editor) else {
        editor.status_message = Some("No word under cursor".into());
        return;
    };
    // Build a `\bword\b` regex so `foo` doesn't match `foobar`.
    let pattern = format!(r"\b{}\b", regex::escape(&word));
    if let Err(e) = editor.search.set_pattern(&pattern) {
        editor.status_message = Some(format!("E: bad pattern: {e}"));
        return;
    }
    editor.search.direction_forward = forward;
    // Record where we are NOW so <C-o> can come back.
    editor.jumplist_record_here();
    // Don't start AT the cursor (we'd hit the current symbol); skip past it.
    crate::search_actions::jump_match(editor, forward, false);
}

fn word_under_cursor(editor: &Editor) -> Option<String> {
    let win = editor.active_window()?;
    let buf = editor.buffers.get(&win.buffer)?;
    let tw = editor.config.options.tab_width;
    let line = buf.line_string(win.cursor.row);
    let byte = twidth::col_to_byte(&line, win.cursor.col, tw);
    if byte >= line.len() {
        return None;
    }
    let bytes = line.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    if !is_word(bytes[byte]) {
        return None;
    }
    // Walk backwards from byte to find word start.
    let mut start = byte;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    // Walk forwards to find word end (exclusive).
    let mut end = byte;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    Some(line[start..end].to_string())
}
