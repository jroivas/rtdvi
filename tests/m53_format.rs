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
fn gqq_on_code_without_formatter_reports_no_formatter() {
    // A non-comment line with no LSP and no configured formatter.
    let (mut editor, _f) = open_with_ext("let x=1;\n", ".rs");
    type_keys(&mut editor, "gqq");
    // Buffer unchanged; a status message explains there's no formatter.
    assert_eq!(text(&editor), "let x=1;\n");
    let msg = editor.status_message.as_deref().unwrap_or("");
    assert!(msg.contains("no formatter"), "status: {msg:?}");
}

#[test]
fn gqq_uses_external_formatter_when_configured() {
    // Fake formatter: `tr a-z A-Z` uppercases stdin — proves the selection is
    // piped through and the output replaces it.
    let (mut editor, _f) = open_with_ext("hello world\n", ".rs");
    editor
        .config
        .formatters
        .insert("rust".into(), vec!["tr".into(), "a-z".into(), "A-Z".into()]);
    type_keys(&mut editor, "gqq");
    assert_eq!(text(&editor), "HELLO WORLD\n");
}
