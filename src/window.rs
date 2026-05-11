//! A window: a viewport onto a buffer with a cursor and selection.
//!
//! v1 holds a single window per tab as a leaf; the [`SplitTree`] type is
//! defined here so milestone 5 (splits) can plug in without ripping out
//! `Tab`'s public surface.

use crate::buffer::BufferId;
use crate::cursor::{Cursor, Selection};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u32);

#[derive(Debug)]
pub struct Window {
    pub id: WindowId,
    pub buffer: BufferId,
    pub cursor: Cursor,
    pub selection: Selection,
    /// First visible row of the buffer.
    pub top_line: usize,
    /// First visible display column (for horizontal scrolling).
    pub left_col: usize,
}

impl Window {
    pub fn new(id: WindowId, buffer: BufferId) -> Self {
        Self {
            id,
            buffer,
            cursor: Cursor::default(),
            selection: Selection::None,
            top_line: 0,
            left_col: 0,
        }
    }
}

/// Direction of a split.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// `:split` — stack horizontally (one above the other).
    Horizontal,
    /// `:vsplit` — side by side.
    Vertical,
}

/// Binary split tree. Milestone 1 only constructs `Leaf` variants;
/// milestone 5 will start using `Split`.
#[derive(Debug)]
pub enum SplitTree {
    Leaf(WindowId),
    Split {
        axis: SplitAxis,
        /// Fraction of the parent rectangle allotted to `first`. (0.0, 1.0).
        ratio: f32,
        first: Box<SplitTree>,
        second: Box<SplitTree>,
    },
}

impl SplitTree {
    pub fn leaf(w: WindowId) -> Self {
        SplitTree::Leaf(w)
    }

    /// Yield every window id in left-to-right / top-to-bottom order.
    pub fn windows(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<WindowId>) {
        match self {
            SplitTree::Leaf(w) => out.push(*w),
            SplitTree::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }
}
