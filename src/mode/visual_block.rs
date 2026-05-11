//! Visual-block mode (`<C-v>`). Stub — implemented in M7.
//!
//! Sketch of the value object that will hold rectangle math; lives here so
//! it can be unit-tested without a TUI when M7 lands.

use std::ops::Range;

use crate::keymap::{Key, KeyCode};
use crate::mode::{switch_mode, ModeId};
use crate::Editor;

#[derive(Debug, Clone)]
pub struct BlockEdit {
    pub rows: Range<usize>,
    /// Display column where the inserted text begins.
    pub col: usize,
    pub text: String,
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    if matches!(key.code, KeyCode::Esc) {
        switch_mode(editor, ModeId::Normal);
    }
    let _ = key;
}
