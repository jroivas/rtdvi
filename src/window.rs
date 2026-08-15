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

    /// The inclusive `(top, bot)` row span covered by the current selection,
    /// or `None` when nothing is selected. Used by line-oriented visual
    /// operators (indent, format).
    pub fn selection_row_range(&self) -> Option<(usize, usize)> {
        let lo_hi = |a: usize, b: usize| if a <= b { (a, b) } else { (b, a) };
        Some(match self.selection {
            Selection::None => return None,
            Selection::Char { anchor } => lo_hi(anchor.row, self.cursor.row),
            Selection::Line { anchor_row } => lo_hi(anchor_row, self.cursor.row),
            Selection::Block { anchor } => lo_hi(anchor.row, self.cursor.row),
        })
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

    /// Vim's `zz` — set `top_line` so the cursor sits in the **middle**
    /// of the visible area. Used after `gd`/`gD`/etc. to land jump
    /// targets in a comfortable position instead of pinned to the
    /// bottom row.
    pub fn center_on_cursor(&mut self) {
        let h = self.viewport_h as usize;
        self.top_line = self.cursor.row.saturating_sub(h / 2);
    }

    /// Center the cursor like `center_on_cursor`, but never scroll past the
    /// buffer's end: near the bottom the last line stays pinned to the bottom
    /// row (no blank space below). Used by a counted `{n}G` / `{n}gg` jump so
    /// the target line centers when the buffer is long enough, and otherwise
    /// falls back to a normal downward-jump position. `last_line` is the
    /// buffer's final 0-based row.
    pub fn center_on_cursor_clamped(&mut self, last_line: usize) {
        let h = self.viewport_h as usize;
        if h == 0 {
            return;
        }
        let centered = self.cursor.row.saturating_sub(h / 2);
        let max_top = last_line.saturating_sub(h.saturating_sub(1));
        self.top_line = centered.min(max_top);
    }

    /// Vim's `zt` — scroll so the cursor's line becomes the top row.
    pub fn scroll_top_to_cursor(&mut self) {
        self.top_line = self.cursor.row;
    }

    /// Vim's `zb` — scroll so the cursor's line becomes the bottom row.
    pub fn scroll_bottom_to_cursor(&mut self) {
        let h = self.viewport_h as usize;
        self.top_line = self.cursor.row.saturating_sub(h.saturating_sub(1));
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

    /// Maximize `target` along `axis`: for every split on the path to `target`
    /// whose axis matches, push the divider so the side holding `target` takes
    /// as much space as the layout allows, shrinking the neighbour to the 5%
    /// floor. Splits on the cross axis are left untouched, so `<C-w>_` grows
    /// height without disturbing column widths (and `<C-w>|` vice-versa).
    /// Returns `true` if `target` was found.
    pub fn maximize(&mut self, target: WindowId, axis: SplitAxis) -> bool {
        match self {
            SplitTree::Leaf(w) => *w == target,
            SplitTree::Split { axis: a, ratio, first, second } => {
                if first.maximize(target, axis) {
                    if *a == axis {
                        *ratio = 0.95;
                    }
                    true
                } else if second.maximize(target, axis) {
                    if *a == axis {
                        *ratio = 0.05;
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Resize the window `target` along `axis` by `delta` (rows for
    /// `Horizontal`, columns for `Vertical`) within the region `area`.
    ///
    /// Adjusts the ratio of the *nearest* ancestor split whose axis matches,
    /// moving the boundary the target window shares with its neighbour. Growing
    /// the target shrinks the neighbour and vice-versa. Returns `true` if a
    /// matching split was found and adjusted; `false` (no-op) if the window has
    /// no split along that axis.
    ///
    /// Minimum sizes: panes never drop below **10%** of their split (the
    /// layout's hard floor). Within that, shrinking a vertical pane also
    /// **soft-stops** by tier — a single resize won't take it below the next
    /// of `{20, 10, 5}` strictly below its current width — so a big shrink
    /// from 50 stops at 20; small moves within a tier are free.
    pub fn resize(
        &mut self,
        target: WindowId,
        axis: SplitAxis,
        delta: i32,
        area: ratatui::layout::Rect,
    ) -> bool {
        self.resize_walk(target, axis, delta, area).1
    }

    /// Returns `(found, applied)`.
    fn resize_walk(
        &mut self,
        target: WindowId,
        axis: SplitAxis,
        delta: i32,
        area: ratatui::layout::Rect,
    ) -> (bool, bool) {
        match self {
            SplitTree::Leaf(w) => (*w == target, false),
            SplitTree::Split { axis: a, ratio, first, second } => {
                let node_axis = *a;
                let r = (*ratio).clamp(0.1, 0.9);
                let (fa, sa) = match node_axis {
                    SplitAxis::Horizontal => split_h(area, r),
                    SplitAxis::Vertical => split_v(area, r),
                };
                let (found_f, applied_f) = first.resize_walk(target, axis, delta, fa);
                if found_f {
                    if !applied_f && node_axis == axis {
                        apply_resize(node_axis, ratio, area, delta, true);
                        return (true, true);
                    }
                    return (true, applied_f);
                }
                let (found_s, applied_s) = second.resize_walk(target, axis, delta, sa);
                if found_s {
                    if !applied_s && node_axis == axis {
                        apply_resize(node_axis, ratio, area, delta, false);
                        return (true, true);
                    }
                    return (true, applied_s);
                }
                (false, false)
            }
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

    /// Return the 1-column Rects that represent vertical split borders.
    /// Only vertical splits produce a border; horizontal splits use the
    /// per-window statusline as a natural separator.
    pub fn borders(&self, area: ratatui::layout::Rect) -> Vec<ratatui::layout::Rect> {
        let mut out = Vec::new();
        self.borders_into(area, &mut out);
        out
    }

    fn borders_into(
        &self,
        area: ratatui::layout::Rect,
        out: &mut Vec<ratatui::layout::Rect>,
    ) {
        if let SplitTree::Split { axis, ratio, first, second } = self {
            let r = (*ratio).clamp(0.1, 0.9);
            match axis {
                SplitAxis::Vertical => {
                    let (a, b) = split_v(area, r);
                    // The border occupies the 1-column gap between a and b.
                    let border_x = a.x + a.width;
                    out.push(ratatui::layout::Rect {
                        x: border_x,
                        y: area.y,
                        width: 1,
                        height: area.height,
                    });
                    first.borders_into(a, out);
                    second.borders_into(b, out);
                }
                SplitAxis::Horizontal => {
                    let (a, b) = split_h(area, r);
                    first.borders_into(a, out);
                    second.borders_into(b, out);
                }
            }
        }
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

/// The soft-stop floor for a shrinking vertical pane: the largest tier in
/// `{20, 10, 5}` strictly below its current width, else the hard floor of 1.
fn width_floor(w: i32) -> i32 {
    for t in [20, 10, 5] {
        if w > t {
            return t;
        }
    }
    1
}

/// Adjust `ratio` so the target pane resizes by `delta` along `axis`, within
/// the node's `area`. Honours per-side minimums (see [`SplitTree::resize`]).
fn apply_resize(
    axis: SplitAxis,
    ratio: &mut f32,
    area: ratatui::layout::Rect,
    delta: i32,
    target_in_first: bool,
) {
    let total = match axis {
        SplitAxis::Horizontal => area.height as i32,
        // Vertical splits reserve 1 column for the border separator.
        SplitAxis::Vertical => area.width.saturating_sub(1) as i32,
    };
    if total < 3 {
        return; // too small to meaningfully resize
    }
    let f = ((total as f32) * (*ratio).clamp(0.1, 0.9)).round() as i32;
    let f = f.clamp(1, total - 1);
    let s = total - f;

    let desired_f = if target_in_first { f + delta } else { f - delta };
    if desired_f == f {
        return;
    }
    let hard = match axis {
        SplitAxis::Horizontal => 2, // 1 content row + statusline
        SplitAxis::Vertical => 1,
    };
    let floor_for = |w: i32| match axis {
        SplitAxis::Horizontal => 2,
        SplitAxis::Vertical => width_floor(w),
    };
    // Whichever side is shrinking gets the tiered floor (based on its current
    // size); the growing side only needs the hard minimum.
    let (min_f, min_s) = if desired_f < f {
        (floor_for(f), hard)
    } else {
        (hard, floor_for(s))
    };
    let lo = min_f;
    let hi = total - min_s;
    if lo > hi {
        return; // no room to move the boundary
    }
    let new_f = desired_f.clamp(lo, hi);
    // The layout enforces a 10% floor per pane; keep the ratio inside that so
    // the result we compute matches what gets rendered.
    *ratio = ((new_f as f32) / (total as f32)).clamp(0.1, 0.9);
}

fn split_v(area: ratatui::layout::Rect, ratio: f32) -> (ratatui::layout::Rect, ratatui::layout::Rect) {
    // Reserve 1 column for the visual border separator between panes.
    let w = area.width.saturating_sub(1);
    let left = ((w as f32) * ratio).round() as u16;
    let left = left.max(1).min(w.saturating_sub(1));
    let a = ratatui::layout::Rect { x: area.x, y: area.y, width: left, height: area.height };
    let b = ratatui::layout::Rect {
        x: area.x + left + 1,
        y: area.y,
        width: w - left,
        height: area.height,
    };
    (a, b)
}
