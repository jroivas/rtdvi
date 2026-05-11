//! Visual mode (character-wise, `v`). Motions and visual-only operators
//! are registered through the keymap registry under `ModeId::Visual`.

use crate::keymap::{self, Key, KeyCode, Resolve};
use crate::mode::{switch_mode, try_accumulate_count, ModeId};
use crate::Editor;

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
    match editor.keymap.resolve(ModeId::Visual, &pending) {
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
