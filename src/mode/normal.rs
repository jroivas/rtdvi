//! Normal mode: keymap-driven, with vim-style count accumulation.
//!
//! Count is captured by intercepting digit keys before they reach the trie:
//! - When the pending key sequence is empty, digits go into `pending_count_pre`.
//! - When the pending sequence is a known operator prefix (`d`, `c`, `y`),
//!   digits go into `pending_count_post`. The two counts are multiplied when
//!   the action eventually fires (vim's `2d3w` → 6 words).
//! - `0` is always the line-start motion unless a count is already in progress.

use crate::keymap::{self, Key, KeyCode, Resolve};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // `:` always enters command mode.
    if matches!(key.code, KeyCode::Char(':')) && key.mods.is_empty() {
        editor.command_line.clear();
        editor.clear_pending_count();
        switch_mode(editor, ModeId::Command);
        return;
    }

    if accumulate_count(editor, key) {
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

/// Return true if the key was consumed as a count digit.
fn accumulate_count(editor: &mut Editor, key: Key) -> bool {
    if !key.mods.is_empty() {
        return false;
    }
    let KeyCode::Char(c) = key.code else {
        return false;
    };
    if !c.is_ascii_digit() {
        return false;
    }
    let d = c.to_digit(10).unwrap() as usize;
    let at_start = editor.pending_keys.is_empty();
    let at_op = is_operator_prefix(&editor.pending_keys);

    // A leading zero with no count yet is the line-start motion, not a digit.
    let leading_zero_at_start =
        at_start && d == 0 && editor.pending_count_pre.is_none();
    let leading_zero_after_op =
        at_op && d == 0 && editor.pending_count_post.is_none();

    if at_start && !leading_zero_at_start {
        editor.pending_count_pre =
            Some(editor.pending_count_pre.unwrap_or(0) * 10 + d);
        return true;
    }
    if at_op && !leading_zero_after_op {
        editor.pending_count_post =
            Some(editor.pending_count_post.unwrap_or(0) * 10 + d);
        return true;
    }
    false
}

fn is_operator_prefix(keys: &[Key]) -> bool {
    keys.len() == 1
        && keys[0].mods.is_empty()
        && matches!(
            keys[0].code,
            KeyCode::Char('d') | KeyCode::Char('c') | KeyCode::Char('y')
        )
}
