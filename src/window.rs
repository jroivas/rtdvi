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

    /// Replace the leaf for `target` with a new split, putting `new_id` on the
    /// "first" (top / left) side. Returns the modified tree.
    pub fn split_leaf(self, target: WindowId, axis: SplitAxis, new_id: WindowId) -> Self {
        match self {
            SplitTree::Leaf(w) if w == target => SplitTree::Split {
                axis,
                ratio: 0.5,
                first: Box::new(SplitTree::Leaf(new_id)),
                second: Box::new(SplitTree::Leaf(w)),
            },
            SplitTree::Leaf(_) => self,
            SplitTree::Split { axis: a, ratio, first, second } => SplitTree::Split {
                axis: a,
                ratio,
                first: Box::new(first.split_leaf(target, axis, new_id)),
                second: Box::new(second.split_leaf(target, axis, new_id)),
            },
        }
    }

    /// Remove a leaf, collapsing the parent split into its sibling. Returns
    /// `None` if removing the last leaf in the tree.
    pub fn remove_leaf(self, target: WindowId) -> Option<Self> {
        match self {
            SplitTree::Leaf(w) if w == target => None,
            SplitTree::Leaf(_) => Some(self),
            SplitTree::Split { axis, ratio, first, second } => {
                let f = first.remove_leaf(target);
                let s = second.remove_leaf(target);
                match (f, s) {
                    (None, None) => None,
                    (Some(t), None) | (None, Some(t)) => Some(t),
                    (Some(f), Some(s)) => Some(SplitTree::Split {
                        axis,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                }
            }
        }
    }

    /// Rebalance every split node so that, along its own axis, the children
    /// receive space proportional to how many same-axis leaves each side
    /// contains. Any cross-axis subtree counts as exactly one unit at the
    /// parent's axis, so this matches vim's `<C-w>=` semantics: columns
    /// equalise at the top level, then within each column the rows
    /// equalise among themselves.
    pub fn equalize(&mut self) {
        if let SplitTree::Split { axis, ratio, first, second } = self {
            let w_first = first.weight_along(*axis) as f32;
            let w_second = second.weight_along(*axis) as f32;
            let total = w_first + w_second;
            if total > 0.0 {
                *ratio = (w_first / total).clamp(0.05, 0.95);
            }
            first.equalize();
            second.equalize();
        }
    }

    /// How many distinct same-axis leaves this subtree contains. A subtree
    /// whose root is a split along a *different* axis counts as 1 (it's a
    /// single "cell" at the caller's axis).
    fn weight_along(&self, axis: SplitAxis) -> usize {
        match self {
            SplitTree::Leaf(_) => 1,
            SplitTree::Split { axis: a, first, second, .. } => {
                if *a == axis {
                    first.weight_along(axis) + second.weight_along(axis)
                } else {
                    1
                }
            }
        }
    }

    /// Lay out the tree into rectangles. Each leaf gets the `Rect` it'll be
    /// rendered into.
    pub fn layout(&self, area: ratatui::layout::Rect) -> Vec<(WindowId, ratatui::layout::Rect)> {
        let mut out = Vec::new();
        self.layout_into(area, &mut out);
        out
    }

    fn layout_into(
        &self,
        area: ratatui::layout::Rect,
        out: &mut Vec<(WindowId, ratatui::layout::Rect)>,
    ) {
        match self {
            SplitTree::Leaf(w) => out.push((*w, area)),
            SplitTree::Split { axis, ratio, first, second } => {
                let r = (*ratio).clamp(0.1, 0.9);
                let (a, b) = match axis {
                    SplitAxis::Horizontal => split_h(area, r),
                    SplitAxis::Vertical => split_v(area, r),
                };
                first.layout_into(a, out);
                second.layout_into(b, out);
            }
        }
    }
}

fn split_h(area: ratatui::layout::Rect, ratio: f32) -> (ratatui::layout::Rect, ratatui::layout::Rect) {
    let h = area.height;
    let top = ((h as f32) * ratio).round() as u16;
    let top = top.max(1).min(h.saturating_sub(1));
    let a = ratatui::layout::Rect { x: area.x, y: area.y, width: area.width, height: top };
    let b = ratatui::layout::Rect {
        x: area.x,
        y: area.y + top,
        width: area.width,
        height: h - top,
    };
    (a, b)
}

fn split_v(area: ratatui::layout::Rect, ratio: f32) -> (ratatui::layout::Rect, ratatui::layout::Rect) {
    let w = area.width;
    let left = ((w as f32) * ratio).round() as u16;
    let left = left.max(1).min(w.saturating_sub(1));
    let a = ratatui::layout::Rect { x: area.x, y: area.y, width: left, height: area.height };
    let b = ratatui::layout::Rect {
        x: area.x + left,
        y: area.y,
        width: w - left,
        height: area.height,
    };
    (a, b)
}
