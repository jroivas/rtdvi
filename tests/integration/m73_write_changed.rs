//! `:w` refuses to overwrite a file that changed on disk; `:w!` forces it.

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

#[test]
fn write_refuses_when_file_changed_on_disk() {
    let (mut editor, f) = open("hello\n");
    std::fs::write(f.path(), "changed externally\n").unwrap();

    run_ex_line(&mut editor, "w");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("changed"),
        "status should warn about external change: {:?}",
        editor.status_message
    );
    // The file on disk should still have the external content.
    assert_eq!(
        std::fs::read_to_string(f.path()).unwrap(),
        "changed externally\n"
    );
}

#[test]
fn write_bang_forces_overwrite() {
    let (mut editor, f) = open("hello\n");
    std::fs::write(f.path(), "changed externally\n").unwrap();

    run_ex_line(&mut editor, "w!");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("written"),
        "status should confirm write: {:?}",
        editor.status_message
    );
    // The file on disk should now have the buffer content.
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "hello\n");
}

#[test]
fn write_quit_bang_forces_overwrite_and_quits() {
    let (mut editor, f) = open("hello\n");
    std::fs::write(f.path(), "changed externally\n").unwrap();

    run_ex_line(&mut editor, "wq!");
    assert!(
        editor.status_message.is_none() || editor.status_message.as_deref().unwrap().contains("written"),
        "status should confirm write: {:?}",
        editor.status_message
    );
    assert!(editor.should_quit);
}

#[test]
fn wqa_refuses_when_file_changed_on_disk() {
    let (mut editor, f) = open("hello\n");
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get_mut(&id).unwrap().insert(0, "x"); // make dirty
    std::fs::write(f.path(), "changed externally\n").unwrap();

    run_ex_line(&mut editor, "wqa");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("changed"),
        "status should warn about external change: {:?}",
        editor.status_message
    );
    assert!(!editor.should_quit, "should not quit when write is refused");
    assert_eq!(
        std::fs::read_to_string(f.path()).unwrap(),
        "changed externally\n"
    );
}

#[test]
fn wqa_bang_forces_overwrite_and_quits() {
    let (mut editor, f) = open("hello\n");
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get_mut(&id).unwrap().insert(0, "x"); // make dirty
    std::fs::write(f.path(), "changed externally\n").unwrap();

    run_ex_line(&mut editor, "wqa!");
    assert!(
        editor.status_message.is_none() || editor.status_message.as_deref().unwrap().contains("written"),
        "status should confirm write: {:?}",
        editor.status_message
    );
    assert!(editor.should_quit);
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), "xhello\n");
}

#[test]
fn write_refuses_when_file_removed_externally() {
    let (mut editor, f) = open("hello\n");
    // The tempfile is dropped on remove; keep the path around.
    let path = f.path().to_path_buf();
    std::fs::remove_file(&path).unwrap();

    run_ex_line(&mut editor, "w");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("changed"),
        "status should warn about external removal: {:?}",
        editor.status_message
    );
    assert!(!path.exists(), "file should not have been recreated by refused :w");
    // Force write recreates it.
    run_ex_line(&mut editor, "w!");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn write_refuses_when_new_file_created_externally() {
    let mut editor = Editor::new();
    let path = std::env::temp_dir().join("rtdvi_test_nonexistent_73.txt");
    // Ensure the file does not exist.
    let _ = std::fs::remove_file(&path);
    let id = editor.open_path(&path).unwrap();
    editor.focus_single(id);
    editor.buffers.get_mut(&id).unwrap().insert(0, "new content\n");

    // Someone creates the file externally before we save.
    std::fs::write(&path, "external content\n").unwrap();

    run_ex_line(&mut editor, "w");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("changed"),
        "status should warn about external change: {:?}",
        editor.status_message
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "external content\n"
    );

    // Cleanup.
    let _ = std::fs::remove_file(&path);
}

#[test]
fn write_refuses_when_file_removed_and_recreated_quickly() {
    let (mut editor, f) = open("hello\n");
    let path = f.path().to_path_buf();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, "replacement\n").unwrap();

    run_ex_line(&mut editor, "w");
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("changed"),
        "status should warn about external change: {:?}",
        editor.status_message
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement\n");

    let _ = std::fs::remove_file(&path);
}
