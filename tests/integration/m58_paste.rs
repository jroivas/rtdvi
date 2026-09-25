//! `P` — paste before the cursor (charwise) / above the current line
//! (linewise), the uppercase counterpart to `p`.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
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

fn text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn capital_p_pastes_line_above() {
    let (mut editor, _f) = open("first\nsecond\n");
    // Yank "first", move to "second", paste it ABOVE that line.
    type_keys(&mut editor, "yyjP");
    assert_eq!(text(&editor), "first\nfirst\nsecond\n");
    // Cursor lands on the pasted line (row 1).
    assert_eq!(cursor(&editor), (1, 0));
}

#[test]
fn lowercase_p_still_pastes_below() {
    // Guard against regressions in the shared paste body.
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "yyjp");
    assert_eq!(text(&editor), "first\nsecond\nfirst\n");
}

#[test]
fn capital_p_pastes_charwise_before_cursor() {
    let (mut editor, _f) = open("abc\n");
    // Yank the single char 'a' charwise via visual mode.
    type_keys(&mut editor, "vy");
    // Move to 'c' (col 2) and paste 'a' before it.
    type_keys(&mut editor, "llP");
    assert_eq!(text(&editor), "abac\n");
    // Cursor sits on the pasted char.
    assert_eq!(cursor(&editor), (0, 2));
}

#[test]
fn capital_p_charwise_at_line_start() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "vy"); // yank 'a'
    // Cursor back at col 0; P inserts before the first char.
    type_keys(&mut editor, "P");
    assert_eq!(text(&editor), "aabc\n");
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn capital_p_linewise_at_buffer_top() {
    let (mut editor, _f) = open("only\n");
    type_keys(&mut editor, "yyP");
    assert_eq!(text(&editor), "only\nonly\n");
    assert_eq!(cursor(&editor), (0, 0));
}
