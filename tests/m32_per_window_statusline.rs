//! Each split has its own statusline; the active one is highlighted.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, ui, Editor};
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

fn render_to_strings(editor: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut out = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut s = String::new();
        for x in 0..area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        out.push(s);
    }
    out
}

#[test]
fn single_window_has_one_statusline_above_cmdline() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);

    let rows = render_to_strings(&mut editor, 60, 6);
    // Row 0..3: buffer content + tilde markers.
    // Row 4: statusline (file name visible).
    // Row 5: cmdline (empty).
    let basename = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let status_row = &rows[4];
    assert!(
        status_row.contains(&basename),
        "expected statusline to show {basename}; row={status_row:?}"
    );
    // The cmdline is the last row; it should not contain the file name.
    assert!(
        !rows[5].contains(&basename),
        "cmdline row should be empty; got {:?}",
        rows[5]
    );
}

#[test]
fn vertical_split_gives_each_window_its_own_statusline() {
    let mut a = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(a, "fn alpha() {{}}").unwrap();
    a.flush().unwrap();
    let mut b = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(b, "fn beta() {{}}").unwrap();
    b.flush().unwrap();

    let mut editor = Editor::new();
    let buf_a = editor.open_path(a.path()).unwrap();
    editor.focus_single(buf_a);
    // Open the second file in a vertical split. After :vsplit + :e <path>,
    // the FOCUSED window (left after vsplit) gets the new buffer.
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    let cmd = format!(":e {}", b.path().display());
    type_keys(&mut editor, &cmd);
    press(&mut editor, KeyCode::Enter);

    let rows = render_to_strings(&mut editor, 80, 6);
    let name_a = a.path().file_name().unwrap().to_string_lossy().to_string();
    let name_b = b.path().file_name().unwrap().to_string_lossy().to_string();
    // Row 4 = statusline for both side-by-side windows. Both filenames
    // should appear on it.
    let status_row = &rows[4];
    assert!(
        status_row.contains(&name_a),
        "missing {name_a} from statusline row {status_row:?}"
    );
    assert!(
        status_row.contains(&name_b),
        "missing {name_b} from statusline row {status_row:?}"
    );
}

#[test]
fn horizontal_split_puts_statusline_between_windows() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn one() {{}}").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);

    let rows = render_to_strings(&mut editor, 60, 10);
    let basename = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    // Top half ~ rows 0..5, bottom half ~ rows 5..9, cmdline row 9.
    // Each half has its own status row. We expect TWO occurrences of the
    // basename in the rendered grid (one per status line).
    let occurrences: usize = rows
        .iter()
        .take(9) // exclude cmdline
        .map(|r| r.matches(&basename).count())
        .sum();
    assert_eq!(
        occurrences, 2,
        "expected the filename on each window's status; got {occurrences}\n{:?}",
        rows
    );
}

#[test]
fn active_window_status_uses_distinct_style() {
    let mut tmp = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn one() {{}}").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();

    // Find both status rows by scanning for the basename.
    let basename = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let area = *buf.area();
    let mut status_rows: Vec<u16> = Vec::new();
    for y in 0..(area.height - 1) {
        let mut row = String::new();
        for x in 0..area.width {
            row.push_str(buf[(x, y)].symbol());
        }
        if row.contains(&basename) {
            status_rows.push(y);
        }
    }
    assert_eq!(status_rows.len(), 2);
    // The two status rows must use distinct background colors — active has
    // White bg, inactive DarkGray.
    let bgs: Vec<Color> = status_rows.iter().map(|&y| buf[(0, y)].bg).collect();
    assert_ne!(
        bgs[0], bgs[1],
        "active and inactive statuslines should look different"
    );
}
