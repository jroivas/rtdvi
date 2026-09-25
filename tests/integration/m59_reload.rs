//! `:e` / `:e!` — reload the current buffer from disk.

use std::io::Write;

use rtdvi::command::run_ex_line;
use rtdvi::Editor;
use tempfile::NamedTempFile;

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
fn dirty(editor: &Editor) -> bool {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().is_dirty()
}

#[test]
fn e_reloads_external_changes() {
    let (mut editor, f) = open("old\n");
    std::fs::write(f.path(), "new content\nline2\n").unwrap();
    run_ex_line(&mut editor, "e");
    assert_eq!(text(&editor), "new content\nline2\n");
    assert!(!dirty(&editor));
}

#[test]
fn e_refuses_when_buffer_modified() {
    let (mut editor, f) = open("disk\n");
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get_mut(&id).unwrap().insert(0, "X"); // make it dirty
    std::fs::write(f.path(), "external\n").unwrap();

    run_ex_line(&mut editor, "e");
    // Buffer is left untouched and an E37 message is shown.
    assert_eq!(text(&editor), "Xdisk\n");
    assert!(editor
        .status_message
        .as_deref()
        .unwrap_or("")
        .contains("E37"));
}

#[test]
fn e_bang_forces_reload_discarding_changes() {
    let (mut editor, f) = open("disk\n");
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get_mut(&id).unwrap().insert(0, "X");
    assert!(dirty(&editor));
    std::fs::write(f.path(), "external\n").unwrap();

    run_ex_line(&mut editor, "e!");
    assert_eq!(text(&editor), "external\n");
    assert!(!dirty(&editor));
}

#[test]
fn reload_clamps_cursor_into_shorter_file() {
    let (mut editor, f) = open("l1\nl2\nl3\nl4\nl5\n");
    let wid = editor.tabs[editor.active_tab].active;
    {
        let w = editor.windows.get_mut(&wid).unwrap();
        w.cursor.row = 4;
        w.cursor.col = 1;
    }
    std::fs::write(f.path(), "only\n").unwrap();
    run_ex_line(&mut editor, "e");
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 0, "cursor row clamped to the only remaining line");
    assert!(w.cursor.col <= 3);
}

#[test]
fn reload_clears_undo_history() {
    // After a reload, the discarded edits can't be brought back with `u`.
    let (mut editor, _f) = open("hello\n");
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get_mut(&id).unwrap().insert(0, "ZZ");
    run_ex_line(&mut editor, "e!");
    assert_eq!(text(&editor), "hello\n");
    // Undo should be a no-op now (nothing on the stack).
    let undone = editor.buffers.get_mut(&id).unwrap().undo();
    assert!(undone.is_none());
    assert_eq!(text(&editor), "hello\n");
}

#[test]
fn e_on_scratch_buffer_reports_no_file_name() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    run_ex_line(&mut editor, "e");
    assert!(editor
        .status_message
        .as_deref()
        .unwrap_or("")
        .contains("E32"));
}

#[test]
fn e_with_path_argument_still_opens_that_file() {
    let (mut editor, _f) = open("first\n");
    let mut other = NamedTempFile::new().unwrap();
    other.write_all(b"second file\n").unwrap();
    other.flush().unwrap();
    run_ex_line(&mut editor, &format!("e {}", other.path().display()));
    assert_eq!(text(&editor), "second file\n");
}
