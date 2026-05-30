//! M8: `/` `?` `n` `N` search.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
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

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn slash_jumps_to_first_match() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "/gam");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (2, 0));
}

#[test]
fn n_jumps_to_next_match() {
    let (mut editor, _f) = open("foo bar foo bar\n");
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "n");
    assert_eq!(cursor(&editor), (0, 8));
    type_keys(&mut editor, "n");
    // Wraps around.
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn question_mark_searches_backward() {
    let (mut editor, _f) = open("aaa bbb ccc\n");
    type_keys(&mut editor, "$"); // end of line
    type_keys(&mut editor, "?aaa");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn capital_n_reverses_direction() {
    let (mut editor, _f) = open("x y x y x\n");
    type_keys(&mut editor, "/x");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "nn"); // 0 -> 4 -> 8 (last 'x'? Actually 0,4,8 are x)
    assert_eq!(cursor(&editor), (0, 8));
    type_keys(&mut editor, "N"); // back to 4
    assert_eq!(cursor(&editor), (0, 4));
}

#[test]
fn no_match_sets_status_message() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/xyz");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.status_message.as_deref().unwrap_or("").contains("not found"));
}

#[test]
fn esc_cancels_prompt_without_setting_pattern() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/he");
    press(&mut editor, KeyCode::Esc);
    assert!(editor.search.last_pattern.is_none());
    assert_eq!(cursor(&editor), (0, 0));
}
