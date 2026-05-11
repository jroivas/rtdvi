//! M7: visual-block (<C-v>) with d/y/I/A.

use std::io::Write;

use jvim::keymap::keys::{Key, KeyCode, KeyMods};
use jvim::mode::ModeId;
use jvim::{mode, Editor};
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

fn text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn ctrl_v_enters_visual_block() {
    let (mut editor, _f) = open("abcd\nefgh\n");
    ctrl(&mut editor, 'v');
    assert_eq!(editor.mode, ModeId::VisualBlock);
}

#[test]
fn block_delete_removes_rectangle() {
    let (mut editor, _f) = open("abcdef\nghijkl\nmnopqr\n");
    // <C-v> j l l selects rows 0-1, cols 0-2 (chars "abc"/"ghi")
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jll");
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "def\njkl\nmnopqr\n");
}

#[test]
fn block_yank_captures_rectangle() {
    let (mut editor, _f) = open("abcdef\nghijkl\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jl"); // rows 0-1, cols 0-1
    type_keys(&mut editor, "y");
    assert_eq!(editor.unnamed_register.text, "ab\ngh");
    // Buffer unchanged.
    assert_eq!(text(&editor), "abcdef\nghijkl\n");
}

#[test]
fn block_d_on_single_row_acts_like_x_range() {
    let (mut editor, _f) = open("abcdef\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "ll"); // cols 0-2
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "def\n");
}

#[test]
fn block_capital_i_enters_insert_at_top_left() {
    let (mut editor, _f) = open("abc\ndef\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "j"); // rows 0-1
    type_keys(&mut editor, "I");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    // v1 doesn't replay across rows yet — only top row gets the change.
    assert_eq!(text(&editor), "Xabc\ndef\n");
}

#[test]
fn block_capital_a_enters_insert_after_right_edge() {
    let (mut editor, _f) = open("abc\ndef\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "l"); // cols 0-1
    type_keys(&mut editor, "A");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    // Top row cursor should have been at col 2 (right+1); insert there.
    assert_eq!(text(&editor), "abXc\ndef\n");
}
