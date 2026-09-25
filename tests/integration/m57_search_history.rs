//! `/` and `?` search history: recorded on submit, persisted to disk,
//! browsable with Up/Down, and kept separate from `:` command history.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::{NamedTempFile, TempDir};

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

/// Open a buffer and redirect the search-history file into a temp dir so the
/// test never touches the real `~/.local/state/rtdvi/search_history`.
fn open(content: &str) -> (Editor, NamedTempFile, TempDir) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    let state = TempDir::new().unwrap();
    editor.search_history.file_path = state.path().join("search_history");
    (editor, f, state)
}

#[test]
fn slash_search_recorded_in_history() {
    let (mut editor, _f, _s) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "/gam");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.search_history.entries, vec!["gam".to_string()]);
}

#[test]
fn question_search_recorded_in_history() {
    let (mut editor, _f, _s) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "?bet");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.search_history.entries, vec!["bet".to_string()]);
}

#[test]
fn empty_search_not_recorded() {
    let (mut editor, _f, _s) = open("alpha\n");
    type_keys(&mut editor, "/");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.search_history.entries.is_empty());
}

#[test]
fn consecutive_duplicate_search_collapsed() {
    let (mut editor, _f, _s) = open("foo foo\n");
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.search_history.entries, vec!["foo".to_string()]);
}

#[test]
fn search_history_persisted_to_disk() {
    let (mut editor, _f, _s) = open("one two three\n");
    type_keys(&mut editor, "/two");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "/three");
    press(&mut editor, KeyCode::Enter);

    let on_disk = rtdvi::history::load_entries(&editor.search_history.file_path);
    assert_eq!(on_disk, vec!["two".to_string(), "three".to_string()]);
}

#[test]
fn up_recalls_previous_searches() {
    let (mut editor, _f, _s) = open("apple\nbanana\ncherry\n");
    type_keys(&mut editor, "/apple");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "/banana");
    press(&mut editor, KeyCode::Enter);

    // Re-enter the prompt and walk back through history.
    type_keys(&mut editor, "/");
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Search);
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.search.prompt, "banana");
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.search.prompt, "apple");
    // Down returns toward newer entries.
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.search.prompt, "banana");
}

#[test]
fn down_past_newest_restores_typed_text() {
    let (mut editor, _f, _s) = open("xxyy\n");
    type_keys(&mut editor, "/xxyy");
    press(&mut editor, KeyCode::Enter);

    // Type a prefix the prior entry matches, browse to it, then step back
    // past the newest entry — the originally-typed text is restored.
    type_keys(&mut editor, "/xx");
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.search.prompt, "xxyy");
    press(&mut editor, KeyCode::Down);
    assert_eq!(editor.search.prompt, "xx");
}

#[test]
fn up_filters_by_typed_prefix() {
    let (mut editor, _f, _s) = open("foobar foobaz qux\n");
    for q in ["foobar", "qux", "foobaz"] {
        type_keys(&mut editor, &format!("/{q}"));
        press(&mut editor, KeyCode::Enter);
    }
    // Typing "foo" then Up should only visit entries starting with "foo".
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.search.prompt, "foobaz");
    press(&mut editor, KeyCode::Up);
    assert_eq!(editor.search.prompt, "foobar");
}

#[test]
fn search_history_separate_from_command_history() {
    let (mut editor, _f, _s) = open("hello world\n");
    type_keys(&mut editor, "/world");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.search_history.entries, vec!["world".to_string()]);
    // The `:` command history must not have picked up the search.
    assert!(editor.history.entries.is_empty());
}
