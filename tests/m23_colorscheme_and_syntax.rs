//! Colorscheme loading, `:colorscheme`, syntax highlighting end-to-end.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::syntax::{detect_filetype, Syntax};
use rtdvi::{colorscheme, mode, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

#[test]
fn detect_filetype_for_common_extensions() {
    use std::path::Path;
    assert_eq!(detect_filetype(Path::new("foo.rs")), "rust");
    assert_eq!(detect_filetype(Path::new("foo.py")), "python");
    assert_eq!(detect_filetype(Path::new("foo.toml")), "toml");
    assert_eq!(detect_filetype(Path::new("Cargo.toml")), "toml");
    assert_eq!(detect_filetype(Path::new("Makefile")), "make");
    assert_eq!(detect_filetype(Path::new("README.md")), "markdown");
}

#[test]
fn syntax_highlights_comment_and_string_in_rust() {
    let syn = Syntax::for_path(Some(std::path::Path::new("foo.rs")));
    let tokens = syn.highlight_line(r#"let x = "hi"; // tail"#);
    let groups: Vec<&str> = tokens.iter().map(|(_, g)| g.as_str()).collect();
    assert!(groups.contains(&"String"), "{:?}", groups);
    assert!(groups.contains(&"Comment"), "{:?}", groups);
}

#[test]
fn syntax_keywords_come_from_system_vim_file_when_available() {
    // System has /usr/share/vim/vim*/syntax/rust.vim — we should pick up
    // some Rust keywords. Skip the assertion if the file isn't installed.
    let syn = Syntax::for_path(Some(std::path::Path::new("foo.rs")));
    if syn.keyword_regexes.is_empty() {
        // No system vim installed; can't test this case.
        return;
    }
    let tokens = syn.highlight_line("fn main() { let x = 1; }");
    // Either `fn` or `let` should be highlighted as some Keyword-ish group.
    let found = tokens.iter().any(|(_, g)| {
        g.contains("Keyword") || g == "Statement" || g.contains("Storage")
    });
    assert!(found, "no keyword highlighted: {:?}", tokens);
}

#[test]
fn colorscheme_command_switches_scheme() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    // The myfault2 scheme exists at ./colors/myfault2.vim.
    type_keys(&mut editor, ":colorscheme myfault2");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.colorscheme.name, "myfault2");
    assert!(editor.colorscheme.style_for("Comment").is_some());
}

#[test]
fn colo_alias_works() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    type_keys(&mut editor, ":colo myfault2");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.colorscheme.name, "myfault2");
}

#[test]
fn colorscheme_with_unknown_name_reports_error() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    type_keys(&mut editor, ":colorscheme this_does_not_exist_anywhere");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.status_message.as_deref().unwrap_or("").contains("Cannot find"));
}

#[test]
fn comment_in_rendered_buffer_uses_scheme_style() {
    // Write a small Rust file, load myfault2 (defines Comment=cyan/#80a0ff),
    // render with TestBackend, and check the comment span shows up styled.
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "// hello comment").unwrap();
    writeln!(tmp, "let n = 42;").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let buf = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(buf);
    editor.colorscheme = colorscheme::load("myfault2").unwrap();

    let backend = TestBackend::new(40, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf_grid = terminal.backend().buffer().clone();
    // Find a non-space cell on row 0 that's part of the comment and check
    // its fg style is the scheme's Comment colour.
    let comment_style = editor.colorscheme.style_for("Comment").unwrap();
    let mut saw_comment_color = false;
    for x in 0..buf_grid.area().width {
        let cell = &buf_grid[(x, 0)];
        let s = cell.symbol();
        if !s.is_empty() && s != " " {
            if cell.fg == comment_style.fg.unwrap_or(Color::Reset) {
                saw_comment_color = true;
                break;
            }
        }
    }
    assert!(saw_comment_color, "no comment-coloured cell on row 0");
}

#[test]
fn search_overlay_uses_scheme_search_style() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "alpha bravo charlie").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let buf = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(buf);
    editor.colorscheme = colorscheme::load("myfault2").unwrap();

    // Run a search so editor.search.pattern is set.
    type_keys(&mut editor, "/bravo");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.search.pattern.is_some());

    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf_grid = terminal.backend().buffer().clone();
    let search_style = editor.colorscheme.style_for("Search").unwrap();
    let mut saw = false;
    for x in 0..buf_grid.area().width {
        let cell = &buf_grid[(x, 0)];
        if cell.symbol() == "b" || cell.symbol() == "r" || cell.symbol() == "a" || cell.symbol() == "v" || cell.symbol() == "o" {
            // Anywhere on "bravo" we should see the search bg.
            if cell.bg == search_style.bg.unwrap_or(Color::Reset) && cell.bg != Color::Reset {
                saw = true;
                break;
            }
        }
    }
    assert!(saw, "no Search-styled cell found on rendered line");
}

#[test]
fn set_syntax_off_disables_and_auto_reenables() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "let x = 42; // c").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let buf = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(buf);

    // Auto-detected as rust to start with.
    assert_eq!(editor.syntax_for(buf).filetype, "rust");

    // :set syntax=off → disabled, no highlighting.
    rtdvi::command::run_ex_line(&mut editor, "set syntax=off");
    assert_eq!(editor.syntax_for(buf).filetype, "off");
    assert!(editor.syntax_for(buf).highlight_line("let x = 42;").is_empty());

    // :set syntax=none → also disabled.
    rtdvi::command::run_ex_line(&mut editor, "set syntax=auto");
    assert_eq!(editor.syntax_for(buf).filetype, "rust");
    rtdvi::command::run_ex_line(&mut editor, "set syntax=none");
    assert_eq!(editor.syntax_for(buf).filetype, "off");

    // :set syntax=on / auto → back to auto-detect (rust).
    rtdvi::command::run_ex_line(&mut editor, "set syntax=on");
    assert_eq!(editor.syntax_for(buf).filetype, "rust");
    assert!(!editor.syntax_for(buf).highlight_line("let x = 42;").is_empty());
}
