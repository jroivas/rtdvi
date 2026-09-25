//! When a file ends with `\n` (almost every source file), there must be
//! no empty gap between the last line and the first `~` placeholder.

use std::io::Write;

use rtdvi::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::NamedTempFile;

#[test]
fn opening_test_c_does_not_show_gap_before_tilde() {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    write!(
        tmp,
        "#include <stdio.h>\n\
         \n\
         int main(int argc, char **argv)\n\
         {{\n\
             printf(\"Hello, World!\");\n\
             return 0;\n\
         }}\n"
    )
    .unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);

    // Render onto a 60×12 grid — bigger than the file, so we should see
    // 7 content rows then `~` markers, with NO blank row in between.
    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();

    // Row 6 (0-indexed) is the `}`; row 7 must be `~`, not empty.
    let row7_first = buf[(0, 7)].symbol();
    assert_eq!(
        row7_first, "~",
        "expected `~` directly under the last content line, got {:?}",
        row7_first
    );
}

#[test]
fn buffer_line_count_matches_displayed_lines() {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    write!(tmp, "a\nb\nc\n").unwrap(); // 3 content lines + trailing nl
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    let buf = editor.buffers.get(&id).unwrap();
    assert_eq!(buf.line_count(), 3);
}

#[test]
fn capital_g_lands_on_last_content_line_not_virtual_empty() {
    // Without the fix `G` on "a\nb\nc\n" would land on the virtual empty
    // line at row 3.
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    write!(tmp, "alpha\nbeta\ngamma\n").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    rtdvi::mode::handle_key(&mut editor, rtdvi::keymap::keys::Key::char('G'));
    let row = editor.active_window().unwrap().cursor.row;
    assert_eq!(row, 2);
}
