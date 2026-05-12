//! The statusline shows the buffer's full path (relative or absolute as
//! opened), truncated from the left when the window is too narrow.

use std::io::Write;

use jvim::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::TempDir;

fn render(editor: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut rows = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut s = String::new();
        for x in 0..area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        rows.push(s);
    }
    rows
}

#[test]
fn statusline_shows_full_path_for_nested_file() {
    // Build a nested directory and open the file via an explicit
    // relative path so the buffer stores the relative form.
    let dir = TempDir::new().unwrap();
    let nested_dir = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested_dir).unwrap();
    let file = nested_dir.join("hello.rs");
    let mut f = std::fs::File::create(&file).unwrap();
    writeln!(f, "fn main() {{}}").unwrap();

    let mut editor = Editor::new();
    let id = editor.open_path(&file).unwrap();
    editor.focus_single(id);

    // 200 cols → plenty of room for the absolute path.
    let rows = render(&mut editor, 200, 4);
    // Row 2 is the statusline (window content + status + cmdline = 4 rows).
    let status_row = &rows[2];
    // The status row should contain the FULL path, not just `hello.rs`.
    let full = file.display().to_string();
    assert!(
        status_row.contains(&full),
        "statusline missing full path\nstatus: {status_row:?}\nexpected to contain: {full}"
    );
}

#[test]
fn statusline_truncates_long_paths_keeping_basename_visible() {
    let dir = TempDir::new().unwrap();
    // Construct a deeply nested directory so the absolute path
    // becomes very long.
    let mut deep = dir.path().to_path_buf();
    for _ in 0..15 {
        deep = deep.join("somewhat_longish_segment");
    }
    std::fs::create_dir_all(&deep).unwrap();
    let file = deep.join("hello.rs");
    std::fs::File::create(&file).unwrap();

    let mut editor = Editor::new();
    let id = editor.open_path(&file).unwrap();
    editor.focus_single(id);

    // 40-col narrow window forces left-truncation.
    let rows = render(&mut editor, 40, 4);
    let status_row = &rows[2];
    // Basename must still be visible.
    assert!(status_row.contains("hello.rs"), "status: {status_row:?}");
    // The full path won't fit, so truncation marker should appear.
    assert!(
        status_row.contains('…'),
        "expected `…` truncation marker, got {status_row:?}"
    );
}

#[test]
fn statusline_shows_no_name_for_scratch_buffer() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    let rows = render(&mut editor, 80, 4);
    let status_row = &rows[2];
    assert!(
        status_row.contains("[No Name]"),
        "status: {status_row:?}"
    );
}
