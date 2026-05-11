//! `<C-w>=` rebalances the split tree so columns (and rows within each
//! column) divide their space equally.

use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode, KeyMods};
use jvim::window::{SplitAxis, SplitTree, WindowId};
use jvim::{mode, Editor};
use ratatui::layout::Rect;
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}
fn ctrl(editor: &mut Editor, c: char) {
    mode::handle_key(editor, Key::with(KeyCode::Char(c), KeyMods::CTRL));
}

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn layout(editor: &Editor, width: u16, height: u16) -> Vec<(WindowId, Rect)> {
    editor.tabs[0]
        .tree
        .layout(Rect { x: 0, y: 0, width, height })
}

fn leaf(id: u32) -> SplitTree {
    SplitTree::Leaf(WindowId(id))
}
fn vsplit(first: SplitTree, second: SplitTree, ratio: f32) -> SplitTree {
    SplitTree::Split {
        axis: SplitAxis::Vertical,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}
fn hsplit(first: SplitTree, second: SplitTree, ratio: f32) -> SplitTree {
    SplitTree::Split {
        axis: SplitAxis::Horizontal,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

#[test]
fn three_vertical_splits_equalize_to_thirds() {
    // Build :vsplit :vsplit -> three columns in a row.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs[0].tree.windows().len(), 3);

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");

    // 90 chars wide -> each column should be ~30 (allow ±2 for rounding).
    let layout = layout(&editor, 90, 24);
    let mut widths: Vec<u16> = layout.iter().map(|(_, r)| r.width).collect();
    widths.sort();
    for w in &widths {
        assert!(
            (*w as i32 - 30).abs() <= 2,
            "column width {w} not ~30, all widths {widths:?}"
        );
    }
}

#[test]
fn three_horizontal_splits_equalize_to_thirds() {
    let (mut editor, _f) = open("hi\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs[0].tree.windows().len(), 3);

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");

    let layout = layout(&editor, 80, 24);
    let mut heights: Vec<u16> = layout.iter().map(|(_, r)| r.height).collect();
    heights.sort();
    for h in &heights {
        assert!(
            (*h as i32 - 8).abs() <= 2,
            "row height {h} not ~8, all heights {heights:?}"
        );
    }
}

#[test]
fn user_scenario_3_columns_with_rows_per_column() {
    // Build the exact described layout by hand:
    //   col 1: single window
    //   col 2: 2 horizontal rows
    //   col 3: 3 horizontal rows
    // Skewed initial ratios on purpose — equalize should fix them.
    let tree = vsplit(
        leaf(1),
        vsplit(
            hsplit(leaf(2), leaf(3), 0.2),
            hsplit(leaf(4), hsplit(leaf(5), leaf(6), 0.3), 0.1),
            0.2,
        ),
        0.1,
    );

    let (mut editor, _f) = open("x\n");
    editor.tabs[0].tree = tree;

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");

    // 90 columns × 24 rows → each of 3 columns ≈ 30 wide.
    let layout = layout(&editor, 90, 24);

    // Group rects by their x coordinate (a column).
    let mut by_x: std::collections::BTreeMap<u16, Vec<Rect>> =
        std::collections::BTreeMap::new();
    for (_, r) in &layout {
        by_x.entry(r.x).or_default().push(*r);
    }
    assert_eq!(by_x.len(), 3, "expected 3 columns, got {by_x:?}");

    // Columns should start at ~0, ~30, ~60, each ~30 wide.
    let widths: Vec<u16> = by_x.values().map(|v| v[0].width).collect();
    let xs: Vec<u16> = by_x.keys().copied().collect();
    for &x in &xs {
        let nearest = [0u16, 30, 60]
            .iter()
            .map(|n| (*n as i32 - x as i32).abs())
            .min()
            .unwrap();
        assert!(nearest <= 2, "column starts at {x}, not near 0/30/60: {xs:?}");
    }
    for &w in &widths {
        assert!((w as i32 - 30).abs() <= 2, "column width {w} not ~30");
    }

    // Rows within each column should be equal too.
    for (x, rects) in &by_x {
        let mut heights: Vec<u16> = rects.iter().map(|r| r.height).collect();
        heights.sort();
        let n = rects.len();
        let expected = 24 / n as u16;
        for h in &heights {
            assert!(
                (*h as i32 - expected as i32).abs() <= 2,
                "column at x={x}: row height {h} not ~{expected}, rows {heights:?}"
            );
        }
    }
}

#[test]
fn equalize_does_nothing_on_single_window() {
    let (mut editor, _f) = open("x\n");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");
    let layout = layout(&editor, 80, 24);
    assert_eq!(layout.len(), 1);
    let (_, r) = layout[0];
    assert_eq!(r.width, 80);
    assert_eq!(r.height, 24);
}

#[test]
fn equalize_after_lopsided_setup() {
    // Make a tree where the first split has a 10:90 ratio, then equalize.
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    // Manually skew the ratio.
    if let SplitTree::Split { ratio, .. } = &mut editor.tabs[0].tree {
        *ratio = 0.1;
    }
    let before = layout(&editor, 100, 24);
    assert!(before[0].1.width <= 12 || before[1].1.width <= 12);

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");

    let after = layout(&editor, 100, 24);
    // Both should now be ~50.
    for (_, r) in &after {
        assert!((r.width as i32 - 50).abs() <= 2);
    }
}
