//! `:e ~/file` expands the leading tilde to `$HOME` and opens the real file
//! (regression: it used to open an empty buffer at the literal path `~/file`).
//!
//! This test overrides `$HOME`, so it lives alone in its own test binary
//! (separate process) to avoid racing other tests that read the environment.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::TempDir;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn edit_command_expands_tilde() {
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());
    let mut f = std::fs::File::create(home.path().join("pkg.txt")).unwrap();
    writeln!(f, "hello tilde").unwrap();

    let mut editor = Editor::new();
    let scratch = editor.open_scratch();
    editor.focus_single(scratch);

    type_keys(&mut editor, ":e ~/pkg.txt");
    mode::handle_key(&mut editor, Key::new(KeyCode::Enter));

    // The active buffer is the real file under $HOME, not an empty `~/pkg.txt`.
    assert_eq!(buffer_text(&editor), "hello tilde\n");
    let id = editor.active_buffer_id().unwrap();
    let path = editor.buffers.get(&id).unwrap().path().unwrap().to_path_buf();
    assert_eq!(path, home.path().join("pkg.txt"));
}
