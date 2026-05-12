//! Keymap action: `<leader>m` to toggle the highlight on the word under
//! the cursor. The two ex commands (`:highlight`, `:nohighlight`) live
//! in `command/builtin.rs` alongside the rest.

use std::sync::Arc;

use crate::keymap::{Action, ActionRegistry, KeymapRegistry};
use crate::mode::ModeId;
use crate::text::width as twidth;
use crate::Editor;

pub fn register_all(reg: &mut ActionRegistry) {
    reg.register(
        "highlight_toggle_word_under_cursor",
        Arc::new(toggle_word_under_cursor),
    );
}

/// Bound to `\m` by default (vim's `\` leader + `m`). When the user has
/// changed `options.leader`, they can rebind to `<leader>m` in their
/// own TOML keymap entry — `<leader>` gets expanded by `apply_config`.
pub fn bind_default_keys(reg: &mut KeymapRegistry) {
    reg.bind(
        ModeId::Normal,
        "\\m",
        Action::Builtin("highlight_toggle_word_under_cursor"),
    )
    .unwrap();
}

fn toggle_word_under_cursor(editor: &mut Editor) {
    let _ = editor.take_count();
    let Some(word) = word_under_cursor(editor) else {
        editor.status_message = Some("highlight: no word under cursor".into());
        return;
    };
    match editor.highlights.toggle_word(&word) {
        crate::highlights::ToggleResult::Added(t) => {
            editor.status_message = Some(format!("highlight: +{t}"));
        }
        crate::highlights::ToggleResult::Removed(t) => {
            editor.status_message = Some(format!("highlight: -{t}"));
        }
        crate::highlights::ToggleResult::BadPattern => {
            editor.status_message = Some("highlight: bad pattern".into());
        }
    }
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
    let mut start = byte;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = byte;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    Some(line[start..end].to_string())
}
