//! Normal mode: keymap-driven, with vim-style count accumulation.
//!
//! Counts are captured by [`try_accumulate_count`] before keys reach the trie.
//! Normal mode also enters command mode on `:`.

use crate::keymap::{self, Key, KeyCode, Resolve};
use crate::mode::{switch_mode, try_accumulate_count, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // `r{char}` is in flight — consume the next key as the replacement.
    if crate::replace_actions::try_consume_replacement(editor, key) {
        return;
    }
    // `"<letter>` register prefix — must come before count/trie so the
    // letter isn't matched as a keymap binding.
    if crate::registers::try_consume_key(editor, key) {
        return;
    }
    if matches!(key.code, KeyCode::Char(':')) && key.mods.is_empty() {
        editor.command_line.clear();
        editor.clear_pending_count();
        switch_mode(editor, ModeId::Command);
        return;
    }

    if try_accumulate_count(editor, key, true) {
        return;
    }

    editor.pending_keys.push(key);
    let pending = editor.pending_keys.clone();
    match editor.keymap.resolve(ModeId::Normal, &pending) {
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
