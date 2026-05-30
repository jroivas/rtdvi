//! Milestone 1: open file -> render one window -> :q exits.
//!
//! Drives the editor with synthetic keys and a ratatui `TestBackend` to
//! verify a usable end-to-end slice without touching a real terminal.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, ui, Editor};
use ratatui::backend::TestBackend;
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

fn render_to_string(editor: &mut Editor, terminal: &mut Terminal<TestBackend>) -> String {
    terminal.draw(|f| ui::render(editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn opens_file_and_renders_first_line() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "hello rtdvi").unwrap();
    writeln!(file, "second line").unwrap();
    file.flush().unwrap();

    let mut editor = Editor::new();
    let id = editor.open_path(file.path()).unwrap();
    editor.focus_single(id);

    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = render_to_string(&mut editor, &mut terminal);
    assert!(frame.contains("hello rtdvi"), "frame was:\n{frame}");
    assert!(frame.contains("second line"), "frame was:\n{frame}");
}

#[test]
fn colon_q_quits_clean_buffer() {
    let file = NamedTempFile::new().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(file.path()).unwrap();
    editor.focus_single(id);

    // Type `:q<CR>`.
    type_keys(&mut editor, ":q");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.should_quit);
}

#[test]
fn colon_q_refuses_dirty_buffer() {
    let mut editor = Editor::new();
    let id = editor.open_scratch();
    editor.focus_single(id);
    // Force dirty without going through insert mode.
    editor.buffers.get_mut(&id).unwrap().insert(0, "dirty");

    type_keys(&mut editor, ":q");
    press(&mut editor, KeyCode::Enter);
    assert!(!editor.should_quit);
    assert!(
        editor.status_message.as_deref().unwrap_or("").contains("No write"),
        "status: {:?}",
        editor.status_message
    );

    // :q! works.
    type_keys(&mut editor, ":q!");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.should_quit);
}
