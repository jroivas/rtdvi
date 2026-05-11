//! `/` (forward) and `?` (backward) search prompt.

use crate::keymap::{Key, KeyCode};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    match (key.code, key.mods.is_empty()) {
        (KeyCode::Esc, _) => {
            editor.search.clear_prompt();
            switch_mode(editor, ModeId::Normal);
        }
        (KeyCode::Enter, _) => {
            let pat = std::mem::take(&mut editor.search.prompt);
            editor.search.prompt_cursor = 0;
            if !pat.is_empty() {
                if let Err(e) = editor.search.set_pattern(&pat) {
                    editor.status_message = Some(format!("E: bad pattern: {e}"));
                    switch_mode(editor, ModeId::Normal);
                    return;
                }
                let forward = editor.search.direction_forward;
                switch_mode(editor, ModeId::Normal);
                crate::search_actions::jump_match(editor, forward, true);
            } else {
                switch_mode(editor, ModeId::Normal);
            }
        }
        (KeyCode::Backspace, _) => {
            if editor.search.prompt_cursor > 0 {
                let cur = editor.search.prompt_cursor;
                let prev = editor.search.prompt[..cur]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                editor.search.prompt.replace_range(prev..cur, "");
                editor.search.prompt_cursor = prev;
            } else {
                switch_mode(editor, ModeId::Normal);
            }
        }
        (KeyCode::Char(c), true) => {
            let cur = editor.search.prompt_cursor;
            editor.search.prompt.insert(cur, c);
            editor.search.prompt_cursor = cur + c.len_utf8();
        }
        _ => {}
    }
}
