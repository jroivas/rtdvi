//! M6: visual and visual-line modes with d/y/c/p.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
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

fn text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn v_then_d_deletes_range() {
    let (mut editor, _f) = open("abcdef\n");
    type_keys(&mut editor, "v");
    assert_eq!(editor.mode, ModeId::Visual);
    type_keys(&mut editor, "ll"); // extend selection to col 2 (cdf...wait, c+l+l = col 2)
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "def\n");
    assert_eq!(editor.mode, ModeId::Normal);
}

#[test]
fn v_then_y_yanks_and_p_pastes() {
    let (mut editor, _f) = open("hello world\n");
    type_keys(&mut editor, "vllll"); // select "hello"
    type_keys(&mut editor, "y");
    // Buffer unchanged; cursor returns to anchor.
    assert_eq!(text(&editor), "hello world\n");
    assert_eq!(editor.unnamed_register.text, "hello");
    // Move to end of line then paste after.
    type_keys(&mut editor, "$p");
    assert!(text(&editor).starts_with("hello world"), "buffer was {:?}", text(&editor));
}

#[test]
fn capital_v_selects_whole_line() {
    let (mut editor, _f) = open("line1\nline2\nline3\n");
    type_keys(&mut editor, "V");
    assert_eq!(editor.mode, ModeId::VisualLine);
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "line2\nline3\n");
}

#[test]
fn capital_v_extends_across_lines() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "Vjj"); // select rows 0, 1, 2
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "d\n");
}

#[test]
fn visual_change_enters_insert() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "vllll");
    type_keys(&mut editor, "c");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "HI");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "HI\n");
}

#[test]
fn visual_yank_linewise_then_paste_below() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "Vy"); // yank line 0 linewise
    assert!(editor.unnamed_register.linewise);
    type_keys(&mut editor, "p"); // paste below
    assert_eq!(text(&editor), "first\nfirst\nsecond\n");
}

#[test]
fn esc_leaves_visual_without_modification() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "vl");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, ModeId::Normal);
    assert_eq!(text(&editor), "hello\n");
}
