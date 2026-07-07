//! M3: insert mode, edit actions, :w save. M4: undo/redo.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
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

fn buffer_text(editor: &Editor) -> String {
    let id = editor.active_buffer_id().unwrap();
    editor.buffers.get(&id).unwrap().rope().to_string()
}

#[test]
fn enter_insert_and_type() {
    let (mut editor, _f) = open("abc\n");
    assert_eq!(editor.mode, ModeId::Normal);
    type_keys(&mut editor, "i");
    assert_eq!(editor.mode, ModeId::Insert);
    type_keys(&mut editor, "X");
    assert_eq!(buffer_text(&editor), "Xabc\n");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(editor.mode, ModeId::Normal);
}

#[test]
fn paste_mode_disables_autoindent_on_enter() {
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A"); // append at end of "    foo", now in insert
    assert_eq!(editor.mode, ModeId::Insert);
    // With paste off, Enter copies the 4-space indent.
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "bar");
    assert_eq!(buffer_text(&editor), "    foo\n    bar\n");

    // Undo back to a clean start and try again with paste on.
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A");
    editor.config.options.paste = true;
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "bar");
    // No indent added — text is verbatim.
    assert_eq!(buffer_text(&editor), "    foo\nbar\n");
}

#[test]
fn bracketed_paste_inserts_verbatim() {
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A"); // insert at end of "    foo"
    // A multi-line paste keeps its own indentation; nothing is re-indented
    // even though autoindent is on (paste is handled out-of-band).
    mode::handle_paste(&mut editor, "\n    bar\n        baz");
    assert_eq!(buffer_text(&editor), "    foo\n    bar\n        baz\n");
    // Cursor lands ON the last pasted character ('z'), not one past it.
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 2);
    assert_eq!(cur.col, 10, "cursor should sit on the last pasted char");
}

#[test]
fn bracketed_paste_cursor_on_last_char_single_line() {
    let (mut editor, _f) = open("ab\n");
    type_keys(&mut editor, "i"); // insert at col 0
    mode::handle_paste(&mut editor, "XYZ");
    assert_eq!(buffer_text(&editor), "XYZab\n");
    let cur = editor.active_window().unwrap().cursor;
    // On 'Z' (col 2), not past it (col 3).
    assert_eq!(cur.col, 2);
}

#[test]
fn bracketed_paste_normalizes_cr_line_endings() {
    // Terminals send `\r` (or `\r\n`) as line separators in bracketed paste.
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "i");
    mode::handle_paste(&mut editor, "l1\rl2\rl3\rl4\rl5");
    // Five real lines, LF-separated.
    assert_eq!(buffer_text(&editor), "l1\nl2\nl3\nl4\nl5\n");
    // Cursor ends on the last char of the fifth line, not stuck on line 1.
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 4);
    assert_eq!(cur.col, 1, "cursor on the '5' of l5");
}

#[test]
fn bracketed_paste_normalizes_crlf_line_endings() {
    let (mut editor, _f) = open("\n");
    type_keys(&mut editor, "i");
    mode::handle_paste(&mut editor, "a\r\nb\r\nc");
    assert_eq!(buffer_text(&editor), "a\nb\nc\n");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 2);
    assert_eq!(cur.col, 0, "cursor on 'c'");
}

#[test]
fn bracketed_paste_with_trailing_newline_lands_on_last_visible_char() {
    let (mut editor, _f) = open("x\n");
    type_keys(&mut editor, "A"); // end of "x"
    mode::handle_paste(&mut editor, "ab\n");
    // 'a','b' appended to make "xab", then newline. Cursor on 'b'.
    assert_eq!(buffer_text(&editor), "xab\n\n");
    let cur = editor.active_window().unwrap().cursor;
    assert_eq!(cur.row, 0);
    assert_eq!(cur.col, 2, "cursor on 'b', skipping the trailing newline");
}

#[test]
fn paste_commands_toggle_option() {
    let (mut editor, _f) = open("x\n");
    assert!(!editor.config.options.paste, "default is nopaste");
    rtdvi::command::run_ex_line(&mut editor, "paste");
    assert!(editor.config.options.paste);
    rtdvi::command::run_ex_line(&mut editor, "nopaste");
    assert!(!editor.config.options.paste);
}

#[test]
fn append_after_cursor() {
    let (mut editor, _f) = open("ab\n");
    // 'l' to move to col 1, then 'a' (insert after) -> insert at col 2
    type_keys(&mut editor, "la");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abX\n");
}

#[test]
fn open_line_below() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "o");
    type_keys(&mut editor, "new");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "first\nnew\nsecond\n");
}

#[test]
fn open_line_above() {
    let (mut editor, _f) = open("first\nsecond\n");
    type_keys(&mut editor, "O");
    type_keys(&mut editor, "head");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "head\nfirst\nsecond\n");
}

#[test]
fn insert_enter_splits_line() {
    let (mut editor, _f) = open("abcd\n");
    type_keys(&mut editor, "ll"); // col 2 (on 'c')
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Enter);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "ab\ncd\n");
}

#[test]
fn backspace_joins_lines() {
    let (mut editor, _f) = open("ab\ncd\n");
    // Move to start of "cd" via j0
    type_keys(&mut editor, "j0");
    type_keys(&mut editor, "i");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "abcd\n");
}

#[test]
fn colon_w_saves_to_file() {
    let (mut editor, file) = open("hello\n");
    type_keys(&mut editor, "A");
    type_keys(&mut editor, " world");
    press(&mut editor, KeyCode::Esc);
    type_keys(&mut editor, ":w");
    press(&mut editor, KeyCode::Enter);
    let on_disk = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(on_disk, "hello world\n");
}

#[test]
fn undo_redo_roundtrip() {
    let (mut editor, _f) = open("abc\n");
    type_keys(&mut editor, "i");
    type_keys(&mut editor, "X");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "Xabc\n");
    type_keys(&mut editor, "u");
    assert_eq!(buffer_text(&editor), "abc\n");
    // C-r to redo
    mode::handle_key(&mut editor, Key::with(KeyCode::Char('r'), KeyMods::CTRL));
    assert_eq!(buffer_text(&editor), "Xabc\n");
}

// ---- autoindent / smartindent tests ----------------------------------------

#[test]
fn autoindent_copies_indent_on_enter() {
    // "    int i;" — Enter should copy the 4-space indent
    let (mut editor, _f) = open("    int i;\n");
    // move to end of line, enter insert, press Enter
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(buffer_text(&editor), "    int i;\n    \n");
}

#[test]
fn autoindent_o_copies_indent() {
    let (mut editor, _f) = open("    int i;\n");
    type_keys(&mut editor, "o"); // open line below
                                 // While inserting, the new line carries the 4-space indent.
    {
        let text = buffer_text(&editor);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], "    ");
    }
    // Leaving the still-blank line with Esc discards the auto-indent (vim).
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "");
}

#[test]
fn smartindent_c_for_loop() {
    // "    for (i = 0; i < 10; i++)" → next line should be +1 level
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        "); // 8 spaces
}

#[test]
fn smartindent_c_if() {
    let (mut editor, _f) = open("    if (x > 0)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        ");
}

#[test]
fn smartindent_open_brace_dedents() {
    // After smartindented line (8 spaces), typing '{' should dedent to 4
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    // cursor is now on 8-space line; type '{'
    type_keys(&mut editor, "{");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "    {");
}

#[test]
fn smartindent_brace_no_dedent_mid_line() {
    // Typing '{' after non-whitespace content should NOT dedent
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A");
    type_keys(&mut editor, "{");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "    foo{");
}

#[test]
fn smartindent_O_copies_indent_only() {
    // O should NOT add extra smartindent even on a control-keyword line
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++)\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "O"); // open line above
                                 // Same level as `for`, not +1 — checked before the blank line is left.
    {
        let text = buffer_text(&editor);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "    ");
    }
    // Esc on the still-blank line discards the auto-indent (vim).
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "");
}

#[test]
fn smartindent_python_colon() {
    let (mut editor, _f) = open("    if True:\n");
    type_keys(&mut editor, ":set syntax=python");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Enter);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "        ");
}

#[test]
fn smartindent_close_brace_dedents() {
    // After autoindented line (8 spaces), typing '}' should dedent to 4
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++) {\n        atoi();\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    // Move to second line (atoi) and open below
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "o"); // new line: 8 spaces (same level as atoi)
    type_keys(&mut editor, "}");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2], "    }");
}

#[test]
fn smart_backspace_snaps_to_tab_stop() {
    // On a blank line with 8 spaces, Backspace should snap to 4 (one shiftwidth)
    let (mut editor, _f) = open("    for (i = 0; i < 10; i++) {\n        atoi();\n");
    type_keys(&mut editor, ":set syntax=c");
    press(&mut editor, KeyCode::Enter);
    type_keys(&mut editor, "j");
    type_keys(&mut editor, "o"); // new line with 8-space indent
    press(&mut editor, KeyCode::Backspace);
    // Checked while still inserting: a subsequent Esc would strip the blank
    // auto-indent line, so we assert the snap before leaving the line.
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2], "    "); // 4 spaces, not 7
}

#[test]
fn smart_backspace_non_whitespace_regular() {
    // Backspace after normal text still removes one character
    let (mut editor, _f) = open("    foo\n");
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Backspace);
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "    fo");
}

// ---- blank auto-indent stripping (vim autoindent) --------------------------

#[test]
fn o_then_esc_leaves_no_trailing_whitespace() {
    // `o` on an indented line auto-indents the new line; pressing Esc without
    // typing anything must leave a truly empty line, not one full of spaces.
    let (mut editor, _f) = open("        test3(i);\n");
    type_keys(&mut editor, "o");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "        test3(i);\n\n");
}

#[test]
fn enter_on_blank_autoindent_line_strips_it() {
    // Reproduces the reported bug: `o`, then Enter (leaving the first new line
    // blank), then type content on the following line. The middle line must be
    // empty — vim discards auto-indent you never used.
    let (mut editor, _f) = open("        test3(i);\n");
    type_keys(&mut editor, "o"); // line 2: 8-space auto-indent
    press(&mut editor, KeyCode::Enter); // leaves line 2 blank, opens line 3
    type_keys(&mut editor, "// something");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(
        buffer_text(&editor),
        "        test3(i);\n\n        // something\n"
    );
}

#[test]
fn autoindent_kept_when_content_typed() {
    // When real content is typed on the auto-indented line, the indent stays.
    let (mut editor, _f) = open("        test3(i);\n");
    type_keys(&mut editor, "obar");
    press(&mut editor, KeyCode::Esc);
    assert_eq!(buffer_text(&editor), "        test3(i);\n        bar\n");
}

#[test]
fn preexisting_whitespace_line_not_stripped_on_esc() {
    // A blank-but-spaced line the user navigates to (no auto-indent this
    // session) must not be clobbered when leaving insert mode elsewhere.
    let (mut editor, _f) = open("    \nfoo\n");
    // Enter insert at end of the spaced line, type nothing, Esc. No autoindent
    // was inserted, so the original 4 spaces must remain.
    type_keys(&mut editor, "A");
    press(&mut editor, KeyCode::Esc);
    let text = buffer_text(&editor);
    assert_eq!(text.lines().next(), Some("    "));
}
