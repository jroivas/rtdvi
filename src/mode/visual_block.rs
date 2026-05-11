//! Visual-block mode (`<C-v>`). Keymap-driven like Visual/VisualLine.

use std::ops::Range;

use crate::keymap::{self, Key, KeyCode, Resolve};
use crate::mode::{switch_mode, try_accumulate_count, ModeId};
use crate::Editor;

/// Value object describing a rectangle of inserted text — used by future
/// block-insert replay (`I` / `A` / `c` propagation across rows).
#[derive(Debug, Clone)]
pub struct BlockEdit {
    pub rows: Range<usize>,
    pub col: usize,
    pub text: String,
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    if matches!(key.code, KeyCode::Esc) {
        if let Some(w) = editor.active_window_mut() {
            w.selection = crate::cursor::Selection::None;
        }
        editor.clear_pending_count();
        switch_mode(editor, ModeId::Normal);
        return;
    }
    if try_accumulate_count(editor, key, false) {
        return;
    }
    editor.pending_keys.push(key);
    let pending = editor.pending_keys.clone();
    match editor.keymap.resolve(ModeId::VisualBlock, &pending) {
        Resolve::Matched(action) => {
            editor.pending_keys.clear();
            keymap::dispatch_action(editor, &action);
        }
        Resolve::Pending => {}
        Resolve::None => {
            editor.pending_keys.clear();
            editor.clear_pending_count();
        }
    }
}
