//! Normal mode: keymap-driven.

use crate::keymap::{self, Key, KeyCode, Resolve};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Special case: `:` always enters command mode regardless of bindings.
    if matches!(key.code, KeyCode::Char(':')) && key.mods.is_empty() {
        editor.command_line.clear();
        switch_mode(editor, ModeId::Command);
        return;
    }

    editor.pending_keys.push(key);
    let pending = editor.pending_keys.clone();
    match editor.keymap.resolve(ModeId::Normal, &pending) {
        Resolve::Matched(action) => {
            editor.pending_keys.clear();
            keymap::dispatch_action(editor, &action);
        }
        Resolve::Pending => {
            // Wait for more keys.
        }
        Resolve::None => {
            // Drop the pending sequence; status hint for debug.
            editor.pending_keys.clear();
        }
    }
}
