//! M9: :e, :bnext, :bprev, :tabnew, :tabnext, :tabprev.

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

fn fresh() -> Editor {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    editor
}

// Tests that touch the process-wide CWD serialize on this lock.
static CWD_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn count_bufs_named(editor: &Editor, name: &str) -> usize {
    editor
        .buffers
        .values()
        .filter(|b| b.path().map_or(false, |p| p.ends_with(name)))
        .count()
}

#[test]
fn same_file_by_relative_path_shares_buffer_across_tabs() {
    let _g = CWD_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("test.c"), "int main(void){}\n").unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut editor = Editor::new();
    // First open (as `rvi test.c` would): stores the canonical absolute path.
    let id0 = editor.open_path(std::path::Path::new("test.c")).unwrap();
    editor.focus_single(id0);

    // New tab, then open the SAME file by relative path.
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":vi test.c");
    press(&mut editor, KeyCode::Enter);

    // The new tab must show the same buffer — not a stale duplicate.
    assert_eq!(editor.active_buffer_id(), Some(id0), "tab should reuse the buffer");
    assert_eq!(count_bufs_named(&editor, "test.c"), 1, "no duplicate buffer");
}

#[test]
fn tabnew_with_filename_reuses_open_buffer() {
    let _g = CWD_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("test.c"), "x\n").unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut editor = Editor::new();
    let id0 = editor.open_path(std::path::Path::new("test.c")).unwrap();
    editor.focus_single(id0);

    type_keys(&mut editor, ":tabnew test.c");
    press(&mut editor, KeyCode::Enter);

    assert_eq!(editor.active_buffer_id(), Some(id0));
    assert_eq!(count_bufs_named(&editor, "test.c"), 1);
}

#[test]
fn edit_in_new_tab_reflects_via_shared_buffer() {
    let _g = CWD_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("test.c"), "int main(void){}\n").unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut editor = Editor::new();
    let id0 = editor.open_path(std::path::Path::new("test.c")).unwrap();
    editor.focus_single(id0);
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":vi test.c");
    press(&mut editor, KeyCode::Enter);

    // Insert in the new tab; the single shared buffer records it.
    type_keys(&mut editor, "istatic ");
    press(&mut editor, KeyCode::Esc);
    let text = editor.buffers.get(&id0).unwrap().rope().to_string();
    assert!(text.starts_with("static "), "edit missing from shared buffer: {text:?}");
}

#[test]
fn e_opens_file_in_active_window() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"loaded content\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = fresh();
    let cmd = format!(":e {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    let id = editor.active_buffer_id().unwrap();
    let text = editor.buffers.get(&id).unwrap().rope().to_string();
    assert_eq!(text, "loaded content\n");
}

#[test]
fn bnext_cycles_through_buffers() {
    let mut editor = fresh();
    let first = editor.active_buffer_id().unwrap();
    // Open another file via :e
    let tmp = NamedTempFile::new().unwrap();
    let cmd = format!(":e {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    let second = editor.active_buffer_id().unwrap();
    assert_ne!(first, second);
    // :bnext should wrap back to first.
    type_keys(&mut editor, ":bnext");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_buffer_id().unwrap(), first);
}

#[test]
fn tabnew_creates_new_tab() {
    let mut editor = fresh();
    assert_eq!(editor.tabs.len(), 1);
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.len(), 2);
    assert_eq!(editor.active_tab, 1);
}

#[test]
fn tabnext_and_tabprev_cycle() {
    let mut editor = fresh();
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 2);
    type_keys(&mut editor, ":tabnext");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 0); // wraps
    type_keys(&mut editor, ":tabprev");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_tab, 2);
}

#[test]
fn e_reuses_buffer_for_same_path() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"x\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = fresh();
    let cmd = format!(":e {}", tmp.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    let first_id = editor.active_buffer_id().unwrap();
    // Open another scratch then come back via :e <same path>
    type_keys(&mut editor, ":tabnew");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.active_buffer_id().unwrap(), first_id);
}
