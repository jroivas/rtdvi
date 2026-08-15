//! `/` (forward) and `?` (backward) search prompt.

use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Ctrl-C cancels the search prompt just like Esc, matching insert/replace.
    if key.code == KeyCode::Char('c') && key.mods.contains(KeyMods::CTRL) {
        cancel(editor);
        return;
    }
    if key.code == KeyCode::Esc {
        cancel(editor);
        return;
    }
    // History browsing.
    if key.mods.is_empty() {
        match key.code {
            KeyCode::Up => {
                history_prev(editor);
                return;
            }
            KeyCode::Down => {
                history_next(editor);
                return;
            }
            _ => {}
        }
    }
    // Shared readline-style editing (Ctrl-A/E/B/F/U/K/W/D, word motions, arrows,
    // Home/End, Delete). The search prompt had none of this before — you could
    // only append and Backspace.
    {
        let s = &mut editor.search;
        if let Some(changed) =
            crate::text::line_edit::handle_edit_key(key, &mut s.prompt, &mut s.prompt_cursor)
        {
            if changed {
                editor.search.update_prompt_re();
                crate::search_actions::incsearch_preview(editor);
            }
            return;
        }
    }
    match (key.code, key.mods.is_empty()) {
        (KeyCode::Enter, _) => {
            let pat = std::mem::take(&mut editor.search.prompt);
            editor.search.prompt_cursor = 0;
            editor.search_history.reset_browse();
            // The incremental preview moved the cursor; put it back to the
            // origin so the final jump (and its jumplist entry) start there.
            reset_to_origin(editor);
            editor.search.origin = None;
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
                // Empty pattern repeats the last search (from `/`, `?`, or a
                // word search like `*`/`#`/`£`) in the direction just chosen —
                // `/` forward, `?` backward — matching vim.
                let forward = editor.search.direction_forward;
                switch_mode(editor, ModeId::Normal);
                crate::search_actions::jump_match(editor, forward, false);
            }
        }
        (KeyCode::Backspace, _) => {
            let s = &mut editor.search;
            if crate::text::line_edit::delete_back(&mut s.prompt, &mut s.prompt_cursor) {
                editor.search.update_prompt_re();
                crate::search_actions::incsearch_preview(editor);
            } else {
                // Backspace on an empty prompt cancels the search (restoring
                // the origin), matching the previous behaviour.
                cancel(editor);
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
            crate::search_actions::incsearch_preview(editor);
        }
        _ => {}
    }
}

/// Cancel the prompt: discard input, snap the view back to where the search
/// began (undoing any incremental preview), and return to normal mode.
fn cancel(editor: &mut Editor) {
    reset_to_origin(editor);
    editor.search.origin = None;
    editor.search.clear_prompt();
    editor.search_history.reset_browse();
    switch_mode(editor, ModeId::Normal);
}

/// Move the cursor and scroll back to the recorded search origin (if any),
/// without clearing it.
fn reset_to_origin(editor: &mut Editor) {
    let Some(origin) = editor.search.origin else {
        return;
    };
    let Some(win_id) = editor.tabs.get(editor.active_tab).map(|t| t.active) else {
        return;
    };
    if let Some(w) = editor.windows.get_mut(&win_id) {
        w.cursor.row = origin.row;
        w.cursor.col = origin.col;
        w.cursor.sticky_col = origin.col;
        w.top_line = origin.top_line;
        w.left_col = origin.left_col;
    }
}

/// Replace the search prompt with the given history entry, moving the
/// cursor to the end and refreshing the incremental-match regex + preview.
fn set_prompt(editor: &mut Editor, text: String) {
    editor.search.prompt_cursor = text.len();
    editor.search.prompt = text;
    editor.search.update_prompt_re();
    crate::search_actions::incsearch_preview(editor);
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
