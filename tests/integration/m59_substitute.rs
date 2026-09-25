//! `:[range]s/pat/rep/[flags]` substitution, including the visual-mode `:`
//! prefill of `'<,'>`.

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

/// Run a full `:`-command from Normal mode.
fn ex(editor: &mut Editor, cmd: &str) {
    type_keys(editor, cmd); // leading ':' switches to command mode
    press(editor, KeyCode::Enter);
}

#[test]
fn percent_s_replaces_first_per_line() {
    let (mut editor, _f) = open("foo foo\nfoo\n");
    ex(&mut editor, ":%s/foo/bar/");
    assert_eq!(text(&editor), "bar foo\nbar\n");
}

#[test]
fn percent_s_g_replaces_all() {
    let (mut editor, _f) = open("foo foo\nfoo\n");
    ex(&mut editor, ":%s/foo/bar/g");
    assert_eq!(text(&editor), "bar bar\nbar\n");
}

#[test]
fn bare_s_targets_current_line_only() {
    let (mut editor, _f) = open("aaa\naaa\n");
    // Cursor starts on line 0.
    ex(&mut editor, ":s/a/X/g");
    assert_eq!(text(&editor), "XXX\naaa\n");
}

#[test]
fn numeric_range() {
    let (mut editor, _f) = open("x\nx\nx\nx\n");
    ex(&mut editor, ":2,3s/x/y/");
    assert_eq!(text(&editor), "x\ny\ny\nx\n");
}

#[test]
fn case_insensitive_flag() {
    let (mut editor, _f) = open("Foo foo FOO\n");
    ex(&mut editor, ":%s/foo/x/gi");
    assert_eq!(text(&editor), "x x x\n");
}

#[test]
fn capture_group_references() {
    let (mut editor, _f) = open("a=b\n");
    ex(&mut editor, r":%s/(\w+)=(\w+)/\2=\1/");
    assert_eq!(text(&editor), "b=a\n");
}

#[test]
fn ampersand_is_whole_match() {
    let (mut editor, _f) = open("cat\n");
    ex(&mut editor, r":%s/cat/[&]/");
    assert_eq!(text(&editor), "[cat]\n");
}

#[test]
fn pattern_not_found_reports_and_leaves_buffer() {
    let (mut editor, _f) = open("hello\n");
    ex(&mut editor, ":%s/xyz/abc/");
    assert_eq!(text(&editor), "hello\n");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("Pattern not found"), "status: {msg:?}");
}

#[test]
fn does_not_intercept_set_or_sort() {
    // `:set` must not be swallowed by the substitute detector.
    let (mut editor, _f) = open("hello\n");
    ex(&mut editor, ":set");
    // No substitution happened; buffer unchanged.
    assert_eq!(text(&editor), "hello\n");
}

#[test]
fn colon_in_visual_prefills_range() {
    let (mut editor, _f) = open("a\nb\nc\nd\n");
    // Visual-line select lines 1-2 (rows 0..1), then press `:`.
    type_keys(&mut editor, "Vj");
    type_keys(&mut editor, ":");
    assert_eq!(editor.mode, ModeId::Command);
    assert_eq!(editor.command_line.input, "'<,'>");
    assert_eq!(editor.last_visual_range, Some((0, 1)));
    // Append the substitution and run it — only rows 0..1 are touched.
    type_keys(&mut editor, "s/./X/");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(text(&editor), "X\nX\nc\nd\n");
}

#[test]
fn visual_range_on_subset() {
    let (mut editor, _f) = open("z\nz\nz\nz\n");
    // Select rows 1..2 with visual-line starting on row 1.
    type_keys(&mut editor, "jVj");
    type_keys(&mut editor, ":");
    assert_eq!(editor.last_visual_range, Some((1, 2)));
    type_keys(&mut editor, "s/z/Q/");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(text(&editor), "z\nQ\nQ\nz\n");
}
