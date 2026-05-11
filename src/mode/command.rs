//! `:` command-line prompt.
//!
//! Tiny single-line editor at the bottom of the screen. Enter runs the
//! command; Esc cancels back to normal mode.

use crate::command::run_ex_line;
use crate::keymap::{Key, KeyCode};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

#[derive(Default, Debug, Clone)]
pub struct CommandLineState {
    pub input: String,
    /// Insert position (byte offset into `input`).
    pub cursor: usize,
}

impl CommandLineState {
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    match (key.code, key.mods.is_empty()) {
        (KeyCode::Esc, _) => {
            editor.command_line.clear();
            switch_mode(editor, ModeId::Normal);
        }
        (KeyCode::Enter, _) => {
            let line = std::mem::take(&mut editor.command_line.input);
            editor.command_line.cursor = 0;
            // Return to normal first so commands can switch modes themselves.
            switch_mode(editor, ModeId::Normal);
            run_ex_line(editor, &line);
        }
        (KeyCode::Backspace, _) => {
            if editor.command_line.cursor > 0 {
                let cur = editor.command_line.cursor;
                // Step back one char boundary.
                let prev = editor.command_line.input[..cur]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                editor.command_line.input.replace_range(prev..cur, "");
                editor.command_line.cursor = prev;
            } else {
                // Empty input + backspace → cancel.
                switch_mode(editor, ModeId::Normal);
            }
        }
        (KeyCode::Char(c), true) => {
            let cur = editor.command_line.cursor;
            editor.command_line.input.insert(cur, c);
            editor.command_line.cursor = cur + c.len_utf8();
        }
        _ => {}
    }
}
