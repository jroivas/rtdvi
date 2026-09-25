//! Quit safety: `:q` closes just the active window when multiple windows/tabs
//! are open (only the last one quits); `:qa`/`:wqa` quit everything.

use std::io::Write;

use rtdvi::command::run_ex_line;
use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}
fn dirty(editor: &mut Editor) {
    mode::handle_key(editor, Key::char('i'));
    mode::handle_key(editor, Key::char('x'));
    mode::handle_key(editor, Key::new(KeyCode::Esc));
}
fn nwin(editor: &Editor) -> usize {
    editor.tabs.iter().map(|t| t.tree.windows().len()).sum()
}
fn status(editor: &Editor) -> String {
    editor.status_message.clone().unwrap_or_default()
}

#[test]
fn q_with_multiple_windows_closes_window_not_editor() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "vsplit"); // 2 windows
    assert_eq!(nwin(&editor), 2);
    run_ex_line(&mut editor, "q");
    assert!(!editor.should_quit, "must not quit with another window open");
    assert_eq!(nwin(&editor), 1);
}

#[test]
fn q_with_multiple_tabs_closes_tab_not_editor() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "tabnew"); // second tab, focused
    assert_eq!(editor.tabs.len(), 2);
    run_ex_line(&mut editor, "q");
    assert!(!editor.should_quit);
    assert_eq!(editor.tabs.len(), 1);
}

#[test]
fn q_on_last_window_quits() {
    let mut editor = fresh();
    assert_eq!(nwin(&editor), 1);
    run_ex_line(&mut editor, "q");
    assert!(editor.should_quit);
}

#[test]
fn q_on_last_window_refuses_when_dirty() {
    let mut editor = fresh();
    dirty(&mut editor);
    run_ex_line(&mut editor, "q");
    assert!(!editor.should_quit);
    assert!(status(&editor).contains("No write since last change"));
}

#[test]
fn q_bang_quits_despite_dirty() {
    let mut editor = fresh();
    dirty(&mut editor);
    run_ex_line(&mut editor, "q!");
    assert!(editor.should_quit);
}

#[test]
fn qa_quits_all_windows_at_once() {
    let mut editor = fresh();
    run_ex_line(&mut editor, "vsplit");
    run_ex_line(&mut editor, "vsplit");
    assert_eq!(nwin(&editor), 3);
    run_ex_line(&mut editor, "qa");
    assert!(editor.should_quit, ":qa should quit everything");
}

#[test]
fn qa_refuses_when_dirty_but_bang_forces() {
    let mut editor = fresh();
    dirty(&mut editor);
    run_ex_line(&mut editor, "qa");
    assert!(!editor.should_quit);
    assert!(status(&editor).contains("No write since last change"));
    run_ex_line(&mut editor, "qa!");
    assert!(editor.should_quit);
}

#[test]
fn wqa_writes_all_dirty_then_quits() {
    let mut fa = NamedTempFile::new().unwrap();
    fa.write_all(b"aaa\n").unwrap();
    fa.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(fa.path()).unwrap();
    editor.focus_single(id);
    dirty(&mut editor); // buffer now modified
    assert!(editor.buffers.get(&id).unwrap().is_dirty());
    run_ex_line(&mut editor, "wqa");
    assert!(editor.should_quit);
    assert!(!editor.buffers.get(&id).unwrap().is_dirty(), "buffer saved");
    // The edit was persisted to disk.
    let on_disk = std::fs::read_to_string(fa.path()).unwrap();
    assert!(on_disk.starts_with("x"), "file written: {on_disk:?}");
}
