//! Visual mode (character-wise, `v`). Stub — implemented in M6.

use crate::keymap::{Key, KeyCode};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    if matches!(key.code, KeyCode::Esc) {
        switch_mode(editor, ModeId::Normal);
    }
    let _ = key;
}
