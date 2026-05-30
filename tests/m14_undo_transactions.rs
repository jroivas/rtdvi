//! Undo collapses block ops + insert sessions into single steps.

use std::io::Write;

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
fn block_insert_then_single_u_undoes_all_rows() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jj"); // rows 0-2
    type_keys(&mut editor, "I// ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "// alpha\n// beta\n// gamma\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "alpha\nbeta\ngamma\n");
}

#[test]
fn block_append_then_single_u_undoes_all_rows() {
    let (mut editor, _f) = open("abc\ndef\nghi\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjl"); // rows 0-2, cols 0-1
    type_keys(&mut editor, "A!");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "ab!c\nde!f\ngh!i\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "abc\ndef\nghi\n");
}

#[test]
fn block_delete_then_single_u_restores_rectangle() {
    let (mut editor, _f) = open("abcdef\nghijkl\nmnopqr\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "jjll"); // rows 0-2, cols 0-2
    type_keys(&mut editor, "d");
    assert_eq!(text(&editor), "def\njkl\npqr\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "abcdef\nghijkl\nmnopqr\n");
}

#[test]
fn insert_session_is_one_undo_step() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "iXYZ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "XYZabc\n");
    type_keys(&mut editor, "u"); // one u reverses the whole session
    assert_eq!(text(&editor), "abc\n");
}

#[test]
fn ctrl_r_redoes_the_whole_block_op() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "I> ");
    press(&mut editor, KeyCode::Esc);
    let after_op = text(&editor);
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "alpha\nbeta\n");
    mode::handle_key(&mut editor, Key::with(KeyCode::Char('r'), KeyMods::CTRL));
    assert_eq!(text(&editor), after_op);
}

#[test]
fn second_u_pops_an_earlier_session() {
    let (mut editor, _f) = open("a\nb\n");
    // First session: append "X" on line 0.
    type_keys(&mut editor, "AX");
    press(&mut editor, KeyCode::Esc);
    // Second session: visual-block insert "* " on both lines.
    type_keys(&mut editor, "gg"); // back to row 0
    ctrl(&mut editor, 'v');
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "I* ");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(text(&editor), "* aX\n* b\n");
    // First u: undo the block insert.
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "aX\nb\n");
    // Second u: undo the first append.
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "a\nb\n");
}

#[test]
fn dd_remains_a_single_undo_step() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "dd");
    assert_eq!(text(&editor), "b\nc\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "a\nb\nc\n");
}

#[test]
fn count_dd_single_undo_step() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    type_keys(&mut editor, "3dd"); // single buffer.delete -> single entry
    assert_eq!(text(&editor), "d\n");
    type_keys(&mut editor, "u");
    assert_eq!(text(&editor), "a\nb\nc\nd\n");
}
