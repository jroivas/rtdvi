//! `:resize` / `:vertical resize` — manual split sizing.
//!
//! Panes never drop below 10% of their split (the layout floor). Within that,
//! shrinking a vertical pane soft-stops at tiers {20, 10, 5} before the floor.

use ratatui::layout::Rect;
use rtdvi::window::{SplitAxis, SplitTree, WindowId};

fn vsplit(ratio: f32) -> SplitTree {
    SplitTree::Split {
        axis: SplitAxis::Vertical,
        ratio,
        first: Box::new(SplitTree::Leaf(WindowId(1))),
        second: Box::new(SplitTree::Leaf(WindowId(2))),
    }
}
fn hsplit(ratio: f32) -> SplitTree {
    SplitTree::Split {
        axis: SplitAxis::Horizontal,
        ratio,
        first: Box::new(SplitTree::Leaf(WindowId(1))),
        second: Box::new(SplitTree::Leaf(WindowId(2))),
    }
}
/// `(first, second)` widths (vertical) or heights (horizontal) from the layout.
fn dims(t: &SplitTree, area: Rect, vertical: bool) -> (u16, u16) {
    let l = t.layout(area);
    if vertical {
        (l[0].1.width, l[1].1.width)
    } else {
        (l[0].1.height, l[1].1.height)
    }
}

#[test]
fn vertical_grow_soft_stops_at_tier_then_10pct_floor() {
    // width 151 → 150 columns of pane space (1 reserved for the border).
    let area = Rect { x: 0, y: 0, width: 151, height: 40 };
    let mut t = vsplit(100.0 / 150.0);
    assert_eq!(dims(&t, area, true), (100, 50));

    // Grow left by 50: right would hit 0 but soft-stops at tier 20.
    assert!(t.resize(WindowId(1), SplitAxis::Vertical, 50, area));
    assert_eq!(dims(&t, area, true), (130, 20));

    // Again: tier would say 10, but the 10% floor of 150 is 15, which wins.
    t.resize(WindowId(1), SplitAxis::Vertical, 50, area);
    assert_eq!(dims(&t, area, true), (135, 15));

    // Already at the 90% cap — no further growth.
    t.resize(WindowId(1), SplitAxis::Vertical, 50, area);
    assert_eq!(dims(&t, area, true), (135, 15));
}

#[test]
fn vertical_small_move_within_tier_is_free() {
    let area = Rect { x: 0, y: 0, width: 151, height: 40 };
    let mut t = vsplit(120.0 / 150.0); // 120 | 30
    assert_eq!(dims(&t, area, true), (120, 30));
    // +5 nudges the boundary without hitting any stop.
    t.resize(WindowId(1), SplitAxis::Vertical, 5, area);
    assert_eq!(dims(&t, area, true), (125, 25));
}

#[test]
fn vertical_resize_from_second_pane_grows_it() {
    let area = Rect { x: 0, y: 0, width: 151, height: 40 };
    let mut t = vsplit(100.0 / 150.0); // 100 | 50
                                       // Grow the RIGHT pane by 20: left shrinks.
    t.resize(WindowId(2), SplitAxis::Vertical, 20, area);
    assert_eq!(dims(&t, area, true), (80, 70));
}

#[test]
fn horizontal_resize_respects_min_rows() {
    let area = Rect { x: 0, y: 0, width: 80, height: 40 };
    let mut t = hsplit(0.5); // 20 | 20
    assert_eq!(dims(&t, area, false), (20, 20));
    // Grow the top a lot → bottom clamped to the 10% floor (4 of 40).
    t.resize(WindowId(1), SplitAxis::Horizontal, 100, area);
    assert_eq!(dims(&t, area, false), (36, 4));
}

#[test]
fn resize_is_noop_without_a_matching_axis_split() {
    let area = Rect { x: 0, y: 0, width: 151, height: 40 };
    let mut t = vsplit(0.5);
    // A vertical-only tree has no horizontal split to resize.
    assert!(!t.resize(WindowId(1), SplitAxis::Horizontal, 10, area));
    // ...and a single leaf has nothing to resize either.
    let mut leaf = SplitTree::Leaf(WindowId(9));
    assert!(!leaf.resize(WindowId(9), SplitAxis::Vertical, 10, area));
}
