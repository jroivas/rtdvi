//! Auto-arrange: opening/closing splits equalizes the layout like `<C-w>=`,
//! unless the user has manually resized a split in the tab (`:resize`).

use std::io::Write;

use ratatui::layout::Rect;
use rtdvi::command::run_ex_line;
use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::{mode, Editor};
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
fn ex(editor: &mut Editor, line: &str) {
    run_ex_line(editor, line);
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

/// Sorted window widths for a synthetic `w × h` render of the active tab.
fn widths(editor: &Editor, w: u16, h: u16) -> Vec<u16> {
    let mut ws: Vec<u16> = editor.tabs[0]
        .tree
        .layout(Rect { x: 0, y: 0, width: w, height: h })
        .iter()
        .map(|(_, r)| r.width)
        .collect();
    ws.sort();
    ws
}
fn nwin(editor: &Editor) -> usize {
    editor.tabs[0].tree.windows().len()
}

#[test]
fn opening_splits_auto_equalizes_without_manual_command() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(nwin(&editor), 3);
    // No `<C-w>=` was issued, yet the three columns should be even thirds.
    let ws = widths(&editor, 90, 24);
    for w in &ws {
        assert!((*w as i32 - 30).abs() <= 2, "not thirds: {ws:?}");
    }
}

#[test]
fn closing_split_auto_equalizes_remaining() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter); // three columns
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "c"); // close the active one
    assert_eq!(nwin(&editor), 2);
    // The two survivors should rebalance to even halves.
    let ws = widths(&editor, 100, 24);
    for w in &ws {
        assert!((*w as i32 - 50).abs() <= 2, "not halves: {ws:?}");
    }
}

#[test]
fn resize_disables_auto_equalize() {
    let (mut editor, _f) = open("hello\n");
    editor.last_window_area = (100, 24);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    // Manually shrink the active column.
    ex(&mut editor, "vertical resize -20");
    assert!(
        editor.tabs[0].manually_resized,
        "resize should mark the tab manually resized"
    );
    // Opening another split must NOT re-equalize — the custom sizing stays.
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(nwin(&editor), 3);
    let ws = widths(&editor, 90, 24);
    let spread = *ws.iter().max().unwrap() as i32 - *ws.iter().min().unwrap() as i32;
    assert!(spread > 4, "expected uneven columns (custom resize kept), got {ws:?}");
}

#[test]
fn manual_equalize_reenables_auto() {
    let (mut editor, _f) = open("hello\n");
    editor.last_window_area = (100, 24);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    ex(&mut editor, "vertical resize -20");
    assert!(editor.tabs[0].manually_resized);
    // `<C-w>=` re-enables auto-arranging.
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "=");
    assert!(!editor.tabs[0].manually_resized, "`<C-w>=` should clear the flag");
    // A subsequent split auto-equalizes again.
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let ws = widths(&editor, 90, 24);
    for w in &ws {
        assert!((*w as i32 - 30).abs() <= 2, "should be thirds again: {ws:?}");
    }
}

#[test]
fn closing_to_single_window_resets_manual_flag() {
    let (mut editor, _f) = open("hello\n");
    editor.last_window_area = (100, 24);
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    ex(&mut editor, "vertical resize -20");
    assert!(editor.tabs[0].manually_resized);
    // Collapse back to a single window.
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "c");
    assert_eq!(nwin(&editor), 1);
    assert!(
        !editor.tabs[0].manually_resized,
        "a lone window should reset to auto"
    );
    // Splitting again auto-equalizes.
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let ws = widths(&editor, 100, 24);
    for w in &ws {
        assert!((*w as i32 - 50).abs() <= 2, "halves: {ws:?}");
    }
}
