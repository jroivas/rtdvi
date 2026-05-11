//! Per-keystroke dispatch perf. The main-loop event drain handles
//! batching, but each individual `mode::handle_key` call must itself be
//! cheap so even huge bursts complete promptly.

use std::io::Write;
use std::time::Instant;

use jvim::keymap::keys::Key;
use jvim::{mode, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn open_long_c_file(n_lines: usize) -> (Editor, NamedTempFile) {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    writeln!(tmp, "#include <stdio.h>").unwrap();
    writeln!(tmp, "int main() {{").unwrap();
    for i in 0..n_lines {
        writeln!(tmp, "    printf(\"line {}\\n\");", i).unwrap();
    }
    writeln!(tmp, "    return 0;").unwrap();
    writeln!(tmp, "}}").unwrap();
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    (editor, tmp)
}

#[test]
fn one_thousand_j_keystrokes_dispatch_quickly() {
    let (mut editor, _f) = open_long_c_file(2000);
    let start = Instant::now();
    for _ in 0..1000 {
        mode::handle_key(&mut editor, Key::char('j'));
    }
    let elapsed = start.elapsed();
    // Per-keystroke dispatch should be microseconds. Budget allows 2 ms
    // each in debug, 0.5 ms each in release — generous safety margin.
    let budget_ms = if cfg!(debug_assertions) { 2000 } else { 500 };
    assert!(
        (elapsed.as_millis() as u64) < budget_ms,
        "1000 `j` dispatches took {elapsed:?} (budget {budget_ms}ms)"
    );
}

#[test]
fn drain_style_burst_then_single_render_works() {
    // Simulate what the main loop does: process a burst of events, then
    // render once. Confirms the final state is what we'd expect from a
    // batched key sequence.
    let (mut editor, _f) = open_long_c_file(200);
    // 100 `j` keystrokes — should move cursor 100 lines down (clamped at
    // last content line).
    for _ in 0..100 {
        mode::handle_key(&mut editor, Key::char('j'));
    }
    let row = editor.active_window().unwrap().cursor.row;
    let id = editor.active_buffer_id().unwrap();
    let last = editor
        .buffers
        .get(&id)
        .unwrap()
        .line_count()
        .saturating_sub(1);
    assert_eq!(row, 100.min(last));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let start = Instant::now();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let render_ms = start.elapsed().as_millis();
    let budget = if cfg!(debug_assertions) { 200 } else { 50 };
    assert!(
        render_ms < budget,
        "single post-burst render took {render_ms}ms (budget {budget}ms)"
    );
}
