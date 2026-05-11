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
    /// Last-known viewport size (cells), updated each render so motions can
    /// scroll the cursor into view without re-asking the TUI for size.
    pub viewport_h: u16,
    pub viewport_w: u16,
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
            viewport_h: 24,
            viewport_w: 80,
        }
    }

    /// Adjust `top_line` / `left_col` so the cursor stays visible.
    /// `scrolloff` is the minimum number of rows kept between cursor and the
    /// vertical edge (vim's `scrolloff` option), defaults to 0 in v1.
    pub fn scroll_into_view(&mut self, scrolloff: usize) {
        let h = self.viewport_h as usize;
        let w = self.viewport_w as usize;
        if h > 0 {
            let bottom = self.top_line + h.saturating_sub(1);
            if self.cursor.row < self.top_line.saturating_add(scrolloff) {
                self.top_line = self.cursor.row.saturating_sub(scrolloff);
            } else if self.cursor.row + scrolloff > bottom {
                // Make the cursor sit `scrolloff` rows above the bottom edge.
                // top = cursor.row + scrolloff + 1 - h
                self.top_line = self
                    .cursor
                    .row
                    .saturating_add(scrolloff + 1)
                    .saturating_sub(h);
            }
        }
        if w > 0 {
            let right = self.left_col + w.saturating_sub(1);
            if self.cursor.col < self.left_col {
                self.left_col = self.cursor.col;
            } else if self.cursor.col > right {
                self.left_col = self.cursor.col + 1 - w;
            }
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
