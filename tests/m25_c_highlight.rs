//! End-to-end test: opening a `.c` file shows highlighted text on the
//! rendered grid even with a minimal user colorscheme.

use std::io::Write;

use rtdvi::{colorscheme, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn render(editor: &mut Editor, w: u16, h: u16) -> Vec<Vec<(String, Option<Color>, Option<Color>)>> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut out = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut row = Vec::with_capacity(area.width as usize);
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            row.push((
                cell.symbol().to_string(),
                if cell.fg == Color::Reset { None } else { Some(cell.fg) },
                if cell.bg == Color::Reset { None } else { Some(cell.bg) },
            ));
        }
        out.push(row);
    }
    out
}

fn any_styled_cell(rows: &[Vec<(String, Option<Color>, Option<Color>)>]) -> bool {
    rows.iter().flatten().any(|(s, fg, _)| {
        !s.trim().is_empty() && s != "~" && fg.is_some()
    })
}

#[test]
fn default_editor_has_styled_base_groups() {
    // Without loading any scheme, `Editor::new` now installs vim defaults.
    let editor = Editor::new();
    assert!(editor.colorscheme.style_for("Statement").is_some());
    assert!(editor.colorscheme.style_for("Type").is_some());
    assert!(editor.colorscheme.style_for("Keyword").is_some(), "Keyword should resolve via Statement");
    assert!(editor.colorscheme.style_for("Comment").is_some());
}

#[test]
fn opening_text_c_renders_keyword_highlights() {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    writeln!(tmp, "#include <stdio.h>").unwrap();
    writeln!(tmp, "int main() {{").unwrap();
    writeln!(tmp, "    if (1) return 0;").unwrap();
    writeln!(tmp, "}}").unwrap();
    tmp.flush().unwrap();

    let mut editor = Editor::new();
    // Use the bundled minimal scheme — defaults will still fill in the
    // Statement/Type styles missing from it.
    if let Ok(scheme) = colorscheme::load("myfault2") {
        editor.colorscheme = scheme;
    }
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);

    let grid = render(&mut editor, 60, 8);
    assert!(
        any_styled_cell(&grid),
        "no styled cell after opening a .c file: {grid:?}"
    );
}

#[test]
fn comment_in_c_file_uses_scheme_comment_style() {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    writeln!(tmp, "// header comment").unwrap();
    writeln!(tmp, "int x = 0;").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    if let Ok(scheme) = colorscheme::load("myfault2") {
        editor.colorscheme = scheme;
    }
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);

    let grid = render(&mut editor, 60, 5);
    let comment_fg = editor.colorscheme.style_for("Comment").and_then(|s| s.fg);
    assert!(comment_fg.is_some(), "scheme should have a Comment colour");
    // Some cell on row 0 should have the Comment colour.
    let row0 = &grid[0];
    let found = row0.iter().any(|(s, fg, _)| !s.trim().is_empty() && *fg == comment_fg);
    assert!(found, "no Comment-coloured cell in `// header comment` line");
}

#[test]
fn c_keyword_int_gets_statement_or_type_style() {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    writeln!(tmp, "int answer = 42;").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    let grid = render(&mut editor, 40, 3);

    // The cell at column 0..3 covers "int". One of them should be styled.
    let int_fg = grid[0][0].1.or(grid[0][1].1).or(grid[0][2].1);
    assert!(int_fg.is_some(), "`int` was not highlighted");
}
