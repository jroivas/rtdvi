//! Ctrl-W hjkl: step exactly one column or row, never skip past closer
//! ones. Pick the target row/column by the cursor's vertical / horizontal
//! screen position; on a boundary, pick the rect above / to the left.

use rtdvi::cursor::Cursor;
use rtdvi::window::{SplitAxis, SplitTree, Window, WindowId};
use rtdvi::Editor;

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

/// Set up an editor with the layout:
///   col 1: single window (id 1)
///   col 2: 2 horizontal splits (ids 2, 3)
///   col 3: 3 horizontal splits (ids 4, 5, 6)
/// Columns are equalised; rows within each column are equalised.
fn three_cols_1_2_3() -> Editor {
    let mut editor = Editor::new();
    let buf = editor.open_scratch();
    // Manually wire 6 windows pointing at the same scratch buffer.
    for id in 1..=6u32 {
        let _ = editor.new_window_id(); // bump the counter
        let w = Window::new(WindowId(id), buf);
        editor.windows.insert(WindowId(id), w);
    }
    let tree = vsplit(
        leaf(1),
        vsplit(
            hsplit(leaf(2), leaf(3), 0.5),
            hsplit(leaf(4), hsplit(leaf(5), leaf(6), 0.5), 1.0 / 3.0),
            0.5,
        ),
        1.0 / 3.0,
    );
    editor.tabs.clear();
    editor.tabs.push(rtdvi::tab::Tab {
        tree,
        active: WindowId(1),
        manually_resized: false,
    });
    editor.active_tab = 0;
    editor
}

fn focus(editor: &Editor) -> u32 {
    editor.tabs[0].active.0
}

fn run(editor: &mut Editor, action: &str) {
    let f = editor.actions.lookup(action).unwrap();
    f(editor);
}

fn set_active(editor: &mut Editor, id: u32) {
    editor.tabs[0].active = WindowId(id);
}

/// Place the cursor at `screen_row` within window `id`. Uses the default
/// viewport_h = 24 from `Window::new`.
fn set_cursor_screen_row(editor: &mut Editor, id: u32, screen_row: usize) {
    let w = editor.windows.get_mut(&WindowId(id)).unwrap();
    w.cursor = Cursor { row: screen_row, col: 0, sticky_col: 0 };
    w.top_line = 0;
}

// ---- Don't skip columns ----------------------------------------------------

#[test]
fn right_from_col1_lands_in_col2_not_col3() {
    let mut editor = three_cols_1_2_3();
    // Cursor near top.
    set_cursor_screen_row(&mut editor, 1, 0);
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 2, "should land in col 2 (top row)");
}

#[test]
fn right_from_col2_lands_in_col3() {
    let mut editor = three_cols_1_2_3();
    set_active(&mut editor, 2); // top of col 2
    set_cursor_screen_row(&mut editor, 2, 0);
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 4, "should land in col 3 top row");
}

#[test]
fn left_from_col3_lands_in_col2_not_col1() {
    let mut editor = three_cols_1_2_3();
    set_active(&mut editor, 4); // col 3 top
    set_cursor_screen_row(&mut editor, 4, 0);
    run(&mut editor, "focus_left");
    assert_eq!(focus(&editor), 2, "should land in col 2");
}

// ---- Cursor vertical position picks target row -----------------------------

#[test]
fn cursor_upper_half_of_col1_picks_top_of_col2() {
    let mut editor = three_cols_1_2_3();
    // Cursor at screen row 5 of 24 -> upper half.
    set_cursor_screen_row(&mut editor, 1, 5);
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 2, "top row of col 2");
}

#[test]
fn cursor_lower_half_of_col1_picks_bottom_of_col2() {
    let mut editor = three_cols_1_2_3();
    set_cursor_screen_row(&mut editor, 1, 18); // lower half of 24
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 3, "bottom row of col 2");
}

#[test]
fn cursor_top_third_of_col1_picks_top_of_col3() {
    let mut editor = three_cols_1_2_3();
    set_active(&mut editor, 2);
    set_cursor_screen_row(&mut editor, 2, 0);
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 4);
}

#[test]
fn cursor_middle_third_of_col2_picks_middle_of_col3() {
    let mut editor = three_cols_1_2_3();
    // From col 2 top (row 2), cursor at row 6 of 24 -> ~25% down a half-height window,
    // which projects to ~12.5% of total height — col 3 top third (0..33%).
    // Use col 2 row 3 (bottom half) cursor at row 12 to land mid-col-3.
    set_active(&mut editor, 3);
    set_cursor_screen_row(&mut editor, 3, 0); // cursor at top of col 2 row 3 -> ~50% of screen
    run(&mut editor, "focus_right");
    assert_eq!(focus(&editor), 5, "middle row of col 3");
}

// ---- Boundary case: cursor exactly at split start lands in that pane --------

#[test]
fn cursor_exactly_at_split_boundary_picks_pane_starting_there() {
    let mut editor = three_cols_1_2_3();
    // Col 1 is full height. Col 2 has rows at screen y 0..12 and 12..24 (synthetic).
    // Put the cursor at screen row 12 — the exact start of the lower pane.
    set_cursor_screen_row(&mut editor, 1, 12);
    run(&mut editor, "focus_right");
    // y_ref=500 is the start of window 3's range [500,1000): land there.
    assert_eq!(focus(&editor), 3, "boundary lands in the pane that starts there");
}

// ---- Down/Up navigation respects cursor x ----------------------------------

#[test]
fn down_from_col2_top_picks_col2_bottom() {
    let mut editor = three_cols_1_2_3();
    set_active(&mut editor, 2);
    set_cursor_screen_row(&mut editor, 2, 0);
    run(&mut editor, "focus_down");
    assert_eq!(focus(&editor), 3);
}

#[test]
fn up_from_col3_bottom_picks_col3_middle() {
    let mut editor = three_cols_1_2_3();
    set_active(&mut editor, 6); // col 3 bottom row
    set_cursor_screen_row(&mut editor, 6, 0);
    run(&mut editor, "focus_up");
    assert_eq!(focus(&editor), 5, "middle row of col 3");
}
