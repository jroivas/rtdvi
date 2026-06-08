//! `gq` formatting: comment reflow (built-in) and external-formatter fallback.

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

fn open_with_ext(content: &str, ext: &str) -> (Editor, NamedTempFile) {
    let mut f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
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
fn gqq_reflows_long_comment_to_textwidth() {
    let long = "// aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo\n";
    let (mut editor, _f) = open_with_ext(long, ".rs");
    editor.config.options.textwidth = 20;
    // Cursor on line 0, format current line.
    type_keys(&mut editor, "gqq");
    for line in text(&editor).lines() {
        assert!(line.starts_with("// "), "lost comment marker: {line:?}");
        let w = line.chars().count();
        assert!(w <= 20, "line exceeds textwidth: {line:?} ({w})");
    }
    // All words preserved.
    let joined: String = text(&editor)
        .lines()
        .map(|l| l.trim_start_matches("// "))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(joined, "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo");
}

#[test]
fn visual_gq_joins_short_comment_lines() {
    let src = "# alpha\n# beta\n# gamma\ncode = 1\n";
    let (mut editor, _f) = open_with_ext(src, ".py");
    editor.config.options.textwidth = 80;
    // Visual-line select the three comment lines (rows 0..=2), then gq.
    type_keys(&mut editor, "V"); // visual line, row 0
    press(&mut editor, KeyCode::Down);
    press(&mut editor, KeyCode::Down); // extend to row 2
    type_keys(&mut editor, "gq");
    assert_eq!(text(&editor), "# alpha beta gamma\ncode = 1\n");
}

#[test]
fn gqq_wraps_plain_long_line_to_textwidth() {
    // Plain text, no LSP, no configured formatter → vim's built-in word wrap.
    let long = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo\n";
    let (mut editor, _f) = open_with_ext(long, ".txt");
    editor.config.options.textwidth = 20;
    type_keys(&mut editor, "gqq");
    for line in text(&editor).lines() {
        assert!(line.chars().count() <= 20, "line exceeds textwidth: {line:?}");
    }
    // Wrapped onto multiple lines, no words lost.
    assert!(text(&editor).lines().count() > 1, "long line should wrap");
    let joined = text(&editor).split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(joined, long.trim());
}

#[test]
fn gqq_wraps_indented_text_preserving_indent() {
    let (mut editor, _f) =
        open_with_ext("    one two three four five six seven eight\n", ".txt");
    editor.config.options.textwidth = 16;
    type_keys(&mut editor, "gqq");
    for line in text(&editor).lines() {
        assert!(line.starts_with("    "), "indent not preserved: {line:?}");
        assert!(line.chars().count() <= 16, "too wide: {line:?}");
    }
}

#[test]
fn gqj_formats_current_and_next_line() {
    // Two short prose lines joined and wrapped (fits in one line under width).
    let (mut editor, _f) = open_with_ext("one two\nthree four\nkeep me\n", ".txt");
    editor.config.options.textwidth = 80;
    type_keys(&mut editor, "gqj"); // current + 1 below → rows 0,1
    assert_eq!(text(&editor), "one two three four\nkeep me\n");
}

#[test]
fn gqq_uses_external_formatter_when_configured() {
    // A configured external formatter still wins over the built-in wrap.
    let (mut editor, _f) = open_with_ext("hello world\n", ".rs");
    editor
        .config
        .formatters
        .insert("rust".into(), vec!["tr".into(), "a-z".into(), "A-Z".into()]);
    editor.config.options.textwidth = 5; // would wrap, but formatter takes priority
    type_keys(&mut editor, "gqq");
    assert_eq!(text(&editor), "HELLO WORLD\n");
}

