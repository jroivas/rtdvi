//! `<C-f>` page down and `<C-u>` page up.

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::{mode, Editor};

fn ctrl(editor: &mut Editor, c: char) {
    mode::handle_key(editor, Key::with(KeyCode::Char(c), KeyMods::CTRL));
}

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

/// Build a buffer with `n` short lines and a fresh editor focused on it.
fn editor_with_lines(n: usize) -> Editor {
    let mut content = String::with_capacity(n * 6);
    for i in 0..n {
        content.push_str(&format!("L{:04}\n", i));
    }
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor.buffers.get_mut(&id).unwrap().insert(0, &content);
    // Set viewport_h to something predictable so tests don't depend on the
    // default 24 silently. 10 rows -> page step is 8.
    let w_id = editor.tabs[0].active;
    let w = editor.windows.get_mut(&w_id).unwrap();
    w.viewport_h = 10;
    editor
}

fn cursor_row(editor: &Editor) -> usize {
    editor.active_window().unwrap().cursor.row
}

#[test]
fn ctrl_f_moves_down_about_one_viewport() {
    let mut editor = editor_with_lines(50);
    assert_eq!(cursor_row(&editor), 0);
    ctrl(&mut editor, 'f');
    // viewport_h = 10, step = 10 - 2 = 8
    assert_eq!(cursor_row(&editor), 8);
}

#[test]
fn ctrl_u_moves_up_about_one_viewport() {
    let mut editor = editor_with_lines(50);
    type_keys(&mut editor, "G"); // jump to last line
    let bottom = cursor_row(&editor);
    ctrl(&mut editor, 'u');
    assert!(cursor_row(&editor) <= bottom.saturating_sub(7));
}

#[test]
fn ctrl_f_clamps_at_end_of_buffer() {
    let mut editor = editor_with_lines(5);
    ctrl(&mut editor, 'f');
    // Buffer only has 5 lines; cursor should clamp to last row.
    let id = editor.active_buffer_id().unwrap();
    let last = editor.buffers.get(&id).unwrap().line_count().saturating_sub(1);
    assert_eq!(cursor_row(&editor), last);
}

#[test]
fn ctrl_u_clamps_at_start_of_buffer() {
    let mut editor = editor_with_lines(5);
    ctrl(&mut editor, 'u');
    assert_eq!(cursor_row(&editor), 0);
}

#[test]
fn page_down_then_up_returns_close_to_original() {
    let mut editor = editor_with_lines(60);
    type_keys(&mut editor, "20j"); // row 20
    let start = cursor_row(&editor);
    ctrl(&mut editor, 'f');
    let mid = cursor_row(&editor);
    assert!(mid > start);
    ctrl(&mut editor, 'u');
    let back = cursor_row(&editor);
    assert_eq!(back, start);
}

#[test]
fn count_multiplies_page_step() {
    let mut editor = editor_with_lines(100);
    // 3<C-f> = three pages forward. step per page = 8. so target ~24.
    type_keys(&mut editor, "3");
    ctrl(&mut editor, 'f');
    let r = cursor_row(&editor);
    assert!(
        (20..=30).contains(&r),
        "expected ~24, got {r}"
    );
}

#[test]
fn page_motion_scrolls_top_line_too() {
    let mut editor = editor_with_lines(100);
    ctrl(&mut editor, 'f');
    let w = editor.active_window().unwrap();
    // After <C-f>, cursor is at top_line + 0 or 1 — top_line should have
    // advanced from 0.
    assert!(w.top_line > 0);
}

#[test]
fn pagedown_key_works() {
    let mut editor = editor_with_lines(50);
    mode::handle_key(&mut editor, Key::new(KeyCode::PageDown));
    assert!(cursor_row(&editor) > 0);
}

#[test]
fn pageup_key_works() {
    let mut editor = editor_with_lines(50);
    type_keys(&mut editor, "20j");
    let before = cursor_row(&editor);
    mode::handle_key(&mut editor, Key::new(KeyCode::PageUp));
    assert!(cursor_row(&editor) < before);
}
