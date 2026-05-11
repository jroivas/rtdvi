//! Modes.
//!
//! v1 keeps modes pragmatic: a `ModeId` tag on `Editor` and a free-function
//! `handle_key` dispatcher per mode. A trait-based dispatch was considered
//! but creates borrow-checker pain when modes mutate the editor. The
//! [`ModeId`] is the seam — plugins later can replace key handling per
//! mode through the keymap registry without needing to add new modes.

pub mod command;
pub mod insert;
pub mod normal;
pub mod search;
pub mod visual;
pub mod visual_block;
pub mod visual_line;

use crate::keymap::Key;
use crate::Editor;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModeId {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
    Command,
    Search,
}

impl ModeId {
    pub fn short_name(self) -> &'static str {
        match self {
            ModeId::Normal => "NORMAL",
            ModeId::Insert => "INSERT",
            ModeId::Visual => "VISUAL",
            ModeId::VisualLine => "V-LINE",
            ModeId::VisualBlock => "V-BLOCK",
            ModeId::Command => "COMMAND",
            ModeId::Search => "SEARCH",
        }
    }
}

pub fn handle_key(editor: &mut Editor, key: Key) {
    match editor.mode {
        ModeId::Normal => normal::handle_key(editor, key),
        ModeId::Insert => insert::handle_key(editor, key),
        ModeId::Visual => visual::handle_key(editor, key),
        ModeId::VisualLine => visual_line::handle_key(editor, key),
        ModeId::VisualBlock => visual_block::handle_key(editor, key),
        ModeId::Command => command::handle_key(editor, key),
        ModeId::Search => search::handle_key(editor, key),
    }
}

/// Switch modes, emitting `ModeChanged`. Free function so callers don't
/// need to borrow `editor` twice.
pub fn switch_mode(editor: &mut Editor, to: ModeId) {
    let from = editor.mode;
    if from == to {
        return;
    }
    editor.mode = to;
    crate::event::emit(editor, crate::event::Event::ModeChanged { from, to });
}
