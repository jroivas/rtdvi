//! M3: insert mode, edit actions, :w save. M4: undo/redo.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::mode::ModeId;
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

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn enter_insert_and_type() {
    let (mut editor, _f) = open("abc\n");
    assert_eq!(editor.mode, ModeId::Normal);
    type_keys(&mut editor, "i");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "X");
    assert_eq!(buffer_text(&editor), "Xabc\n");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, ModeId::Normal);
}

#[test]
fn append_after_cursor() {
    let (mut editor, _f) = open("ab\n");
    // 'l' to move to col 1, then 'a' (insert after) -> insert at col 2
    type_keys(&mut editor, "la");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abX\n");
}

#[test]
fn open_line_below() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "o");
    type_keys(&mut editor, "new");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "first\nnew\nsecond\n");
}

#[test]
fn open_line_above() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "O");
    type_keys(&mut editor, "head");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "head\nfirst\nsecond\n");
}

#[test]
fn insert_enter_splits_line() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "ll"); // col 2 (on 'c')
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Enter);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "ab\ncd\n");
}

#[test]
fn backspace_joins_lines() {
    let (mut editor, _f) = open("ab\ncd\n");
    // Move to start of "cd" via j0
    type_keys(&mut editor, "j0");
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abcd\n");
}

#[test]
fn colon_w_saves_to_file() {
    let (mut editor, file) = open("hello\n");
    type_keys(&mut editor, "A");
    type_keys(&mut editor, " world");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, ":w");
    press(&mut editor, KeyCode::Enter);
    let on_disk = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(on_disk, "hello world\n");
}

#[test]
fn undo_redo_roundtrip() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "i");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "Xabc\n");
    type_keys(&mut editor, "u");
    assert_eq!(buffer_text(&editor), "abc\n");
    // C-r to redo
    mode::handle_key(&mut editor, Key::with(KeyCode::Char('r'), KeyMods::CTRL));
    assert_eq!(buffer_text(&editor), "Xabc\n");
}

// ---- autoindent / smartindent tests ----------------------------------------

#[test]
fn autoindent_copies_indent_on_enter() {
    // "    int i;" — Enter should copy the 4-space indent
    let (mut editor, _f) = open("    int i;\n");
    // move to end of line, enter insert, press Enter
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(buffer_text(&editor), "    int i;\n    \n");
}

#[test]
fn autoindent_o_copies_indent() {
    let (mut editor, _f) = open("    int i;\n");
    type_keys(&mut editor, "o"); // open line below
    press(&mut editor, KeyCode::Esc);
    // new line should have 4-space indent
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "    ");
}

#[test]
fn smartindent_c_for_loop() {
    // "    for (i = 0; i < 10; i++)" → next line should be +1 level
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        "); // 8 spaces
}

#[test]
fn smartindent_c_if() {
    let (mut editor, _f) = open("    if (x > 0)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        ");
}

#[test]
fn smartindent_open_brace_dedents() {
    // After smartindented line (8 spaces), typing '{' should dedent to 4
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    // cursor is now on 8-space line; type '{'
    type_keys(&mut editor, "{");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "    {");
}

#[test]
fn smartindent_brace_no_dedent_mid_line() {
    // Typing '{' after non-whitespace content should NOT dedent
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A");
    type_keys(&mut editor, "{");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "    foo{");
}

#[test]
fn smartindent_O_copies_indent_only() {
    // O should NOT add extra smartindent even on a control-keyword line
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "O"); // open line above
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "    "); // same level as `for`, not +1
}

#[test]
fn smartindent_python_colon() {
    let (mut editor, _f) = open("    if True:\n");
    type_keys(&mut editor, ":set syntax=python");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        ");
}

#[test]
fn smartindent_close_brace_dedents() {
    // After autoindented line (8 spaces), typing '}' should dedent to 4
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++) {\n        atoi();\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    // Move to second line (atoi) and open below
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "o"); // new line: 8 spaces (same level as atoi)
    type_keys(&mut editor, "}");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2], "    }");
}

#[test]
fn smart_backspace_snaps_to_tab_stop() {
    // On a blank line with 8 spaces, Backspace should snap to 4 (one shiftwidth)
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++) {\n        atoi();\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "o"); // new line with 8-space indent
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2], "    "); // 4 spaces, not 7
}

#[test]
fn smart_backspace_non_whitespace_regular() {
    // Backspace after normal text still removes one character
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "    fo");
}
