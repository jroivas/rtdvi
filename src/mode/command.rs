//! `:` command-line prompt.
//!
//! Tiny single-line editor at the bottom of the screen. Enter runs the
//! command; Esc cancels back to normal mode. `:ff <query>` activates an
//! interactive fuzzy file finder — see the ff_* helpers below.

use crate::command::run_ex_line;
use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

#[derive(Default, Debug, Clone)]
pub struct CommandLineState {
    pub input: String,
    /// Insert position (byte offset into `input`).
    pub cursor: usize,
    /// Active Tab-completion cycle, if any. Cleared by any non-Tab key.
    pub completion: Option<crate::completion::CompletionState>,
}

impl CommandLineState {
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.completion = None;
    }
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    // Tab opens / cycles the completion popup.
    if matches!(key.code, KeyCode::Tab) && key.mods.is_empty() {
        crate::completion::handle_tab(editor);
        return;
    }

    // `:ff` interactive popup takes priority over the regular completion
    // popup. If the user is typing `:ff …`, arrow keys / Enter / Esc go
    // to the fuzzy finder; everything else falls through to text editing
    // so they can refine the query.
    let ff_active = editor.fzf_state.is_some();
    if ff_active && key.mods.is_empty() {
        match key.code {
            KeyCode::Up => {
                ff_nav(editor, -1);
                return;
            }
            KeyCode::Down => {
                ff_nav(editor, 1);
                return;
            }
            KeyCode::Esc => {
                editor.fzf_state = None;
                editor.command_line.clear();
                switch_mode(editor, ModeId::Normal);
                return;
            }
            KeyCode::Enter => {
                ff_accept_and_open(editor);
                return;
            }
            _ => {}
        }
    }

    // While the regular completion popup is visible, arrow keys steer it.
    let popup = editor
        .command_line
        .completion
        .as_ref()
        .map_or(false, |c| c.popup_visible);
    if popup && key.mods.is_empty() {
        match key.code {
            KeyCode::Up => {
                crate::completion::nav_up(editor);
                return;
            }
            KeyCode::Down => {
                crate::completion::nav_down(editor);
                return;
            }
            KeyCode::Left => {
                crate::completion::close_popup(editor);
                return;
            }
            KeyCode::Right => {
                crate::completion::accept(editor);
                return;
            }
            KeyCode::Enter => {
                crate::completion::accept(editor);
            }
            _ => {}
        }
    }

    // History browsing: Up/Down when no completion popup and no ff session.
    if !popup && !ff_active && key.mods.is_empty() {
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

    // Any other key invalidates the current Tab-completion cycle.
    editor.command_line.completion = None;

    // Cancel: Esc or Ctrl-C.
    let is_cancel = key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.mods.contains(KeyMods::CTRL));
    if is_cancel {
        editor.fzf_state = None;
        editor.command_line.clear();
        editor.history.reset_browse();
        editor.shell_filter_range = None;
        switch_mode(editor, ModeId::Normal);
        return;
    }

    // Shared readline-style line editing (Ctrl-A/E/B/F/U/K/W/D, word motions,
    // arrows, Home/End, Delete) — identical on macOS and Linux.
    {
        let cl = &mut editor.command_line;
        if let Some(changed) =
            crate::text::line_edit::handle_edit_key(key, &mut cl.input, &mut cl.cursor)
        {
            if changed {
                ff_refresh_if_active(editor);
            }
            return;
        }
    }

    match (key.code, key.mods.is_empty()) {
        (KeyCode::Enter, _) => {
            // ff_accept_and_open handles Enter above when ff is active.
            let line = std::mem::take(&mut editor.command_line.input);
            editor.command_line.cursor = 0;
            editor.history.reset_browse();
            if !line.is_empty() {
                editor.history.add(line.clone());
            }
            switch_mode(editor, ModeId::Normal);
            run_ex_line(editor, &line);
        }
        (KeyCode::Backspace, _) => {
            let cl = &mut editor.command_line;
            if !crate::text::line_edit::delete_back(&mut cl.input, &mut cl.cursor) {
                // Backspace on an empty line closes the command line, vim-style.
                switch_mode(editor, ModeId::Normal);
            }
            ff_refresh_if_active(editor);
        }
        // Accept any char that isn't a CTRL combo. AltGr-produced symbols
        // arrive with the ALT modifier set, so gating on `mods.is_empty()`
        // would silently drop them.
        (KeyCode::Char(c), _) if !key.mods.contains(KeyMods::CTRL) => {
            let cur = editor.command_line.cursor;
            editor.command_line.input.insert(cur, c);
            editor.command_line.cursor = cur + c.len_utf8();
            ff_refresh_if_active(editor);
        }
        _ => {}
    }
}

/// If the command line currently looks like `:ff …`, (re)build the
/// fuzzy file index (lazily) and refresh the match list.
fn ff_refresh_if_active(editor: &mut Editor) {
    let input = editor.command_line.input.clone();
    let bang = input.starts_with("ff!");
    let query = match parse_ff_query(&input) {
        Some(q) => q,
        None => {
            editor.fzf_state = None;
            return;
        }
    };
    // Bust the index cache the moment the user transitions into the
    // banged form, not on every keystroke after.
    let was_bang = editor.fzf_state.as_ref().map_or(false, |s| s.bang);
    if bang && !was_bang {
        editor.fzf_index = None;
    }
    if editor.fzf_index.is_none() {
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        editor.fzf_index = Some(crate::fzf::Index::build(&root));
    }
    let matches: Vec<String> = editor
        .fzf_index
        .as_ref()
        .map(|idx| {
            crate::fzf::search(idx, query)
                .into_iter()
                .map(|(p, _)| p)
                .collect()
        })
        .unwrap_or_default();
    let selected = editor
        .fzf_state
        .as_ref()
        .map(|s| s.selected.min(matches.len().saturating_sub(1)))
        .unwrap_or(0);
    editor.fzf_state = Some(crate::editor::FzfSession {
        query: query.to_string(),
        matches,
        selected,
        bang,
    });
}

/// Recognise `:ff` (with optional trailing query, optional `!` to bust
/// the index cache). Returns the query string, or `None` if the input
/// isn't an `:ff` invocation.
fn parse_ff_query(input: &str) -> Option<&str> {
    if let Some(rest) = input.strip_prefix("ff!") {
        Some(rest.trim_start())
    } else if let Some(rest) = input.strip_prefix("ff ") {
        Some(rest)
    } else if input == "ff" {
        Some("")
    } else {
        None
    }
}

fn ff_nav(editor: &mut Editor, delta: i32) {
    let Some(state) = editor.fzf_state.as_mut() else {
        return;
    };
    if state.matches.is_empty() {
        return;
    }
    let n = state.matches.len() as i32;
    let idx = state.selected as i32 + delta;
    let idx = ((idx % n) + n) % n;
    state.selected = idx as usize;
}

fn ff_accept_and_open(editor: &mut Editor) {
    let Some(state) = editor.fzf_state.take() else {
        return;
    };
    let Some(path) = state.matches.get(state.selected).cloned() else {
        editor.fzf_state = None;
        return;
    };
    // Clear the command line, leave command mode, open the file. We let
    // the relative path resolve against the current working directory —
    // same as `:e <path>` would.
    editor.command_line.clear();
    switch_mode(editor, ModeId::Normal);
    let path_buf = std::path::PathBuf::from(&path);
    // Reuse existing buffer if one already references this path.
    let existing = editor
        .buffers
        .iter()
        .find(|(_, b)| b.path() == Some(path_buf.as_path()))
        .map(|(id, _)| *id);
    let buf_id = match existing {
        Some(id) => id,
        None => match editor.open_path(&path_buf) {
            Ok(id) => id,
            Err(e) => {
                editor.status_message = Some(format!("open {} failed: {e}", path));
                return;
            }
        },
    };
    editor.jumplist_record_here();
    if let Some(w) = editor.active_window_mut() {
        w.buffer = buf_id;
        w.cursor = crate::cursor::Cursor::default();
        w.selection = crate::cursor::Selection::None;
        w.top_line = 0;
        w.left_col = 0;
    }
    // Record the pick in command history as the equivalent `:vi <path>`, so the
    // file is reachable by browsing the edit history (`:vi ` then Up) — the ff
    // Enter path otherwise bypasses history entirely.
    editor.history.add(format!("vi {path}"));
    editor.status_message = Some(format!("ff: opened {}", path));
}

fn history_prev(editor: &mut Editor) {
    let current = editor.command_line.input.clone();
    if let Some(entry) = editor.history.prev(&current) {
        let s = entry.to_string();
        editor.command_line.cursor = s.len();
        editor.command_line.input = s;
    }
}

fn history_next(editor: &mut Editor) {
    match editor.history.next() {
        Ok(entry) => {
            let s = entry.to_string();
            editor.command_line.cursor = s.len();
            editor.command_line.input = s;
        }
        Err(()) => {
            let saved = editor.history.saved_input.clone();
            editor.command_line.cursor = saved.len();
            editor.command_line.input = saved;
            editor.history.reset_browse();
        }
    }
}
