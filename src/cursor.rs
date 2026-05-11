//! Cursor and selection types.

use crate::buffer::BufferId;

/// 0-indexed (row, display-column) cursor position.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Desired display column when moving vertically through shorter lines.
    pub sticky_col: usize,
}

impl Cursor {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col, sticky_col: col }
    }
}

/// Selection mode plus anchor. The active cursor lives on the `Window`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    None,
    /// Character-wise visual (`v`).
    Char { anchor: Cursor },
    /// Line-wise visual (`V`).
    Line { anchor_row: usize },
    /// Visual-block (`<C-v>`). Anchor stored in display columns.
    Block { anchor: Cursor },
}

impl Default for Selection {
    fn default() -> Self {
        Selection::None
    }
}

/// Marker tying a selection back to its buffer (anti-stale).
#[derive(Copy, Clone, Debug)]
pub struct SelectionContext {
    pub buffer: BufferId,
    pub selection: Selection,
}
