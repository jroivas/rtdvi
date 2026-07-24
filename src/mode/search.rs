//! `/` (forward) and `?` (backward) search prompt.

use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Ctrl-C cancels the search prompt just like Esc, matching insert/replace.
    if key.code == KeyCode::Char('c') && key.mods.contains(KeyMods::CTRL) {
        editor.search.clear_prompt();
        editor.search_history.reset_browse();
        switch_mode(editor, ModeId::Normal);
        return;
    }
    match (key.code, key.mods.is_empty()) {
        (KeyCode::Esc, _) => {
            editor.search.clear_prompt();
            editor.search_history.reset_browse();
            switch_mode(editor, ModeId::Normal);
        }
        (KeyCode::Up, true) => history_prev(editor),
        (KeyCode::Down, true) => history_next(editor),
        (KeyCode::Enter, _) => {
            let pat = std::mem::take(&mut editor.search.prompt);
            editor.search.prompt_cursor = 0;
            editor.search_history.reset_browse();
            if !pat.is_empty() {
                if let Err(e) = editor.search.set_pattern(&pat) {
                    editor.status_message = Some(format!("E: bad pattern: {e}"));
                    switch_mode(editor, ModeId::Normal);
                    return;
                }
                // Record the submitted pattern in the persistent search history.
                editor.search_history.add(pat.clone());
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
                editor.search.update_prompt_re();
            } else {
                switch_mode(editor, ModeId::Normal);
            }
        }
        // Accept any char that isn't a CTRL combo. AltGr-produced symbols
        // (e.g. `£`) arrive with the ALT modifier set, so gating on
        // `mods.is_empty()` would silently drop them — matches insert mode.
        (KeyCode::Char(c), _) if !key.mods.contains(KeyMods::CTRL) => {
            let cur = editor.search.prompt_cursor;
            editor.search.prompt.insert(cur, c);
            editor.search.prompt_cursor = cur + c.len_utf8();
            editor.search.update_prompt_re();
        }
        _ => {}
    }
}

/// Replace the search prompt with the given history entry, moving the
/// cursor to the end and refreshing the incremental-match regex.
fn set_prompt(editor: &mut Editor, text: String) {
    editor.search.prompt_cursor = text.len();
    editor.search.prompt = text;
    editor.search.update_prompt_re();
}

/// Up at the search prompt: step to an older matching entry.
fn history_prev(editor: &mut Editor) {
    let current = editor.search.prompt.clone();
    if let Some(entry) = editor.search_history.prev(&current) {
        let s = entry.to_string();
        set_prompt(editor, s);
    }
}

/// Down at the search prompt: step to a newer entry, or restore the text
/// the user had typed before browsing once past the newest one.
fn history_next(editor: &mut Editor) {
    match editor.search_history.next() {
        Ok(entry) => {
            let s = entry.to_string();
            set_prompt(editor, s);
        }
        Err(()) => {
            let saved = editor.search_history.saved_input.clone();
            set_prompt(editor, saved);
            editor.search_history.reset_browse();
        }
    }
}
