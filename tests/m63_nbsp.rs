//! `[options] nbsp_marker` — a configurable visual indicator for non-breaking
//! spaces (U+00A0), which otherwise render identically to a normal space.

use std::io::Write;

use rtdvi::config::Config;
use rtdvi::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn open_with(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::with_suffix(".txt").unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

/// (symbol, fg) at each cell of the first row.
fn row0(editor: &mut Editor, w: u16, h: u16) -> Vec<(String, Color)> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..w).map(|x| (buf[(x, 0u16)].symbol().to_string(), buf[(x, 0u16)].fg)).collect()
}

const NBSP: &str = "\u{a0}";
const NBSP_FG: Color = Color::Rgb(0xe5, 0x9b, 0x4c);

#[test]
fn nbsp_renders_as_blank_by_default() {
    let (mut editor, _f) = open_with(&format!("a{NBSP}b\n"));
    let cells = row0(&mut editor, 20, 4);
    // No marker configured → the NBSP char is shown as-is (not substituted).
    assert_eq!(cells[1].0, NBSP);
    assert_ne!(cells[1].1, NBSP_FG);
}

#[test]
fn nbsp_marker_substitutes_glyph_and_colour() {
    let cfg: Config = toml::from_str("[options]\nnbsp_marker = \"·\"\n").unwrap();
    let (mut editor, _f) = open_with(&format!("a{NBSP}b\n"));
    editor.apply_config(cfg);
    let cells = row0(&mut editor, 20, 4);
    assert_eq!(cells[0].0, "a");
    // Column 1 (the NBSP) now shows the marker glyph in the NBSP colour.
    assert_eq!(cells[1].0, "·");
    assert_eq!(cells[1].1, NBSP_FG);
    // A normal space is untouched.
    assert_eq!(cells[2].0, "b");

    // A real space stays a space, even with the marker on.
    let (mut editor2, _f2) = open_with("a b\n");
    editor2.apply_config(toml::from_str("[options]\nnbsp_marker = \"·\"\n").unwrap());
    let cells2 = row0(&mut editor2, 20, 4);
    assert_eq!(cells2[1].0, " ");
    assert_ne!(cells2[1].1, NBSP_FG);
}
