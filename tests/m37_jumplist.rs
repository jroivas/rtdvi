//! Jumplist (`<C-o>` / `<C-i>` / `<Tab>`) and `*` / `£` / `#` symbol
//! search. Recording happens at every jump motion (gd/gG/`/n`/]]/%/*).

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
    let mut f = NamedTempFile::with_suffix(".rs").unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

// ---- <C-o> / <C-i> over G ------------------------------------------------

#[test]
fn ctrl_o_returns_after_capital_g() {
    let (mut editor, _f) = open(&format!("{}\n", "a\n".repeat(50)));
    // Start at row 0.
    type_keys(&mut editor, "G"); // jump to last line
    assert!(cursor(&editor).0 > 0);
    let after_g = cursor(&editor).0;
    ctrl(&mut editor, 'o'); // back
    assert_eq!(cursor(&editor).0, 0);
    // <C-i> via <Tab>.
    press(&mut editor, KeyCode::Tab);
    assert_eq!(cursor(&editor).0, after_g);
}

#[test]
fn ctrl_o_chains_through_deep_history() {
    let (mut editor, _f) = open(&format!("{}\n", "x\n".repeat(50)));
    // gg→G→gg→G→gg→G means we pile up four entries: 0,49,0,49,0.
    type_keys(&mut editor, "G"); // 49
    type_keys(&mut editor, "gg"); // 0
    type_keys(&mut editor, "G"); // 49
    type_keys(&mut editor, "gg"); // 0
    // We're at 0. <C-o> walks back: 49, 0, 49, 0 (the original "from" of
    // each jump), then exhausts.
    ctrl(&mut editor, 'o');
    assert!(cursor(&editor).0 > 0);
    ctrl(&mut editor, 'o');
    ctrl(&mut editor, 'o');
    ctrl(&mut editor, 'o');
    // The jumplist has finite history of recorded positions.
    let entries = editor.jumplist.entries().len();
    assert!(entries >= 4, "expected >=4 jumplist entries, got {entries}");
}

#[test]
fn ctrl_o_at_empty_history_is_noop_with_message() {
    let (mut editor, _f) = open("a\nb\nc\n");
    ctrl(&mut editor, 'o');
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("oldest"), "got: {status:?}");
}

#[test]
fn ctrl_i_at_newest_jump_is_noop_with_message() {
    let (mut editor, _f) = open("a\nb\nc\n");
    type_keys(&mut editor, "G"); // record entry
    press(&mut editor, KeyCode::Tab); // forward — nothing newer
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("newest"), "got: {status:?}");
}

// ---- New jump drops forward history --------------------------------------

#[test]
fn new_jump_after_back_drops_forward_entries() {
    let (mut editor, _f) = open(&format!("{}\n", "x\n".repeat(50)));
    type_keys(&mut editor, "G"); // 49
    ctrl(&mut editor, 'o'); // back to 0
    // Forward history is "49". Do a fresh jump → drops the forward entry.
    type_keys(&mut editor, "20gg"); // row 19
    // <C-i> should now find nothing.
    let before = cursor(&editor);
    press(&mut editor, KeyCode::Tab);
    assert_eq!(cursor(&editor), before, "<C-i> shouldn't have moved");
}

// ---- % match-bracket records too -----------------------------------------

#[test]
fn percent_jump_records_in_jumplist() {
    let (mut editor, _f) = open("fn main() {\n    return 0;\n}\n");
    // Cursor at (0,0). `$` lands on `{` (col 10). `%` jumps to `}` at row 2.
    type_keys(&mut editor, "$");
    type_keys(&mut editor, "%");
    assert_eq!(cursor(&editor), (2, 0));
    ctrl(&mut editor, 'o');
    // Back to the `{`.
    assert_eq!(cursor(&editor).0, 0);
}

// ---- Search records jumps ------------------------------------------------

#[test]
fn search_records_jump_back_position() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\ndelta\n");
    type_keys(&mut editor, "/gamma");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor).0, 2);
    ctrl(&mut editor, 'o');
    assert_eq!(cursor(&editor).0, 0);
}

// ---- * / £ / # symbol search ---------------------------------------------

#[test]
fn star_searches_for_word_under_cursor_forward() {
    let (mut editor, _f) = open("foo bar foo baz foo\n");
    // Cursor at (0,0) on `foo`. `*` should jump to the next `foo` (col 8).
    type_keys(&mut editor, "*");
    assert_eq!(cursor(&editor), (0, 8));
    // Stored pattern should include word boundaries.
    let pat = editor.search.last_pattern.as_deref().unwrap_or("");
    assert!(pat.contains("foo"));
    assert!(pat.contains(r"\b"), "pattern should be word-bounded: {pat:?}");
}

#[test]
fn star_records_jumplist_so_ctrl_o_comes_back() {
    let (mut editor, _f) = open("foo bar baz\nbar foo qux\n");
    // Sit on `foo` at row 0 col 0; `*` jumps to row 1 col 4.
    type_keys(&mut editor, "*");
    assert_eq!(cursor(&editor).0, 1);
    ctrl(&mut editor, 'o');
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn pound_searches_backward() {
    let (mut editor, _f) = open("foo bar foo baz foo qux\n");
    // Move to the last `foo` (col 16) and press `#`.
    type_keys(&mut editor, "$");
    type_keys(&mut editor, "$"); // cursor at end of line
    type_keys(&mut editor, "bb"); // back two words = `foo` at col 16
    type_keys(&mut editor, "#"); // search backward for word
    // Should jump to the previous `foo` at col 8.
    assert_eq!(cursor(&editor), (0, 8));
}

#[test]
fn star_on_non_word_position_is_noop() {
    let (mut editor, _f) = open("   spaces\n");
    type_keys(&mut editor, "*");
    let status = editor.status_message.as_deref().unwrap_or("");
    assert!(status.contains("No word"), "status: {status:?}");
}

#[test]
fn star_records_pattern_in_search_history() {
    let (mut editor, _f) = open("foo bar foo\n");
    // Redirect history file so the test never touches real user state.
    let state = tempfile::tempdir().unwrap();
    editor.search_history.file_path = state.path().join("search_history");
    type_keys(&mut editor, "*");
    // The `\bfoo\b` pattern is now recallable at the `/` prompt.
    assert_eq!(editor.search_history.entries, vec![r"\bfoo\b".to_string()]);
}

#[test]
fn pound_symbol_alias_searches_word_backward() {
    let (mut editor, _f) = open("foo bar foo baz foo qux\n");
    type_keys(&mut editor, "$"); // cursor at end of line
    type_keys(&mut editor, "bb"); // back two words = `foo` at col 16
    // Vim aliases `£` to `#`: search backward for the word under the cursor.
    mode::handle_key(&mut editor, Key::char('£'));
    // Should jump to the previous `foo` at col 8.
    assert_eq!(cursor(&editor), (0, 8));
}

// ---- Cross-buffer jump ---------------------------------------------------

#[test]
fn ctrl_o_can_cross_buffers() {
    let mut tmp_a = NamedTempFile::with_suffix(".rs").unwrap();
    tmp_a.write_all(b"alpha\nalpha2\n").unwrap();
    tmp_a.flush().unwrap();
    let mut tmp_b = NamedTempFile::with_suffix(".rs").unwrap();
    tmp_b.write_all(b"beta\nbeta2\n").unwrap();
    tmp_b.flush().unwrap();
    let mut editor = Editor::new();
    let id_a = editor.open_path(tmp_a.path()).unwrap();
    editor.focus_single(id_a);
    // Move cursor a bit so the "from" position is non-trivial.
    type_keys(&mut editor, "j");
    let path_b = format!(":e {}", tmp_b.path().display());
    type_keys(&mut editor, &path_b);
    press(&mut editor, KeyCode::Enter);
    let id_b = editor.active_buffer_id().unwrap();
    assert_ne!(id_a, id_b);
    // Move in B.
    type_keys(&mut editor, "j");
    ctrl(&mut editor, 'o');
    // Should be back in A.
    assert_eq!(editor.active_buffer_id().unwrap(), id_a);
}
