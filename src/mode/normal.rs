//! Normal mode: keymap-driven, with vim-style count accumulation.
//!
//! Counts are captured by [`try_accumulate_count`] before keys reach the trie.
//! Normal mode also enters command mode on `:`.

use crate::keymap::{self, Key, KeyCode, KeyMods, Resolve};
use crate::mode::{switch_mode, try_accumulate_count, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Location picker is modal: intercept all keys while it is open.
    if editor.lsp_picker.is_some() {
        handle_picker_key(editor, key);
        return;
    }

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

fn handle_picker_key(editor: &mut Editor, key: Key) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') if key.mods.is_empty() => {
            if let Some(p) = editor.lsp_picker.as_mut() {
                let n = p.locations.len();
                p.selected = if p.selected == 0 { n.saturating_sub(1) } else { p.selected - 1 };
            }
        }
        KeyCode::Down | KeyCode::Char('j') if key.mods.is_empty() => {
            if let Some(p) = editor.lsp_picker.as_mut() {
                let n = p.locations.len();
                p.selected = if n == 0 { 0 } else { (p.selected + 1) % n };
            }
        }
        KeyCode::Enter if key.mods.is_empty() => {
            if let Some(p) = editor.lsp_picker.take() {
                if let Some((uri, line, col)) = p.locations.get(p.selected).cloned() {
                    crate::lsp_actions::open_uri_at(editor, &uri, line as usize, col as usize);
                }
            }
        }
        KeyCode::Esc if key.mods.is_empty() => {
            editor.lsp_picker = None;
        }
        KeyCode::Char('q') if key.mods.is_empty() => {
            editor.lsp_picker = None;
        }
        KeyCode::Char('c') if key.mods == KeyMods::CTRL => {
            editor.lsp_picker = None;
        }
        _ => {}
    }
}
