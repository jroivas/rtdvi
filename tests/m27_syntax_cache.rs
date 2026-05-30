//! Performance regression: syntax engine is cached per buffer, so repeated
//! renders don't re-read system .vim files or recompile keyword regexes.

use std::io::Write;
use std::time::Instant;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}

fn open_c_file() -> (Editor, NamedTempFile) {
    let mut tmp = NamedTempFile::with_suffix(".c").unwrap();
    for _ in 0..200 {
        writeln!(tmp, "int main() {{ return 0; }}").unwrap();
    }
    tmp.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(tmp.path()).unwrap();
    editor.focus_single(id);
    (editor, tmp)
}

#[test]
fn syntax_for_returns_same_instance_for_same_buffer() {
    let (editor, _f) = open_c_file();
    let id = editor.active_buffer_id().unwrap();
    let s1 = editor.syntax_for(id);
    let s2 = editor.syntax_for(id);
    // Arc::ptr_eq lets us check they're the SAME underlying Syntax — i.e.
    // the second call was served by the cache.
    assert!(
        std::sync::Arc::ptr_eq(&s1, &s2),
        "syntax_for built a new Syntax instead of reusing the cached one"
    );
}

#[test]
fn set_syntax_invalidates_cache() {
    let (mut editor, _f) = open_c_file();
    let id = editor.active_buffer_id().unwrap();
    let before = editor.syntax_for(id);
    assert_eq!(before.filetype, "c");

    // Change filetype via :set.
    mode::handle_key(&mut editor, Key::char(':'));
    for c in "set syntax=python".chars() {
        mode::handle_key(&mut editor, Key::char(c));
    }
    press(&mut editor, KeyCode::Enter);

    let after = editor.syntax_for(id);
    assert_eq!(after.filetype, "python");
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after),
        "syntax_for returned the stale C engine after `:set syntax=python`"
    );
}

#[test]
fn many_renders_finish_quickly_thanks_to_cache() {
    let (mut editor, _f) = open_c_file();

    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    // Warm-up render — pays the one-time cost of loading the syntax file.
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();

    // Now hammer the renderer. If we were re-reading c.vim on every render
    // this loop would be very slow; with the cache it must stay reasonable.
    // Debug builds have ~10x overhead vs. release for regex matching, so
    // pick the budget based on which we're in. 6s is still well under what
    // an uncached render would take (each uncached render reads the system
    // syntax file and recompiles a dozen regexes — easily 30ms each in
    // debug, so 200 uncached renders ≈ 6s+).
    let n = 200;
    let budget_ms = if cfg!(debug_assertions) { 6000 } else { 2000 };
    let start = Instant::now();
    for _ in 0..n {
        terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        (elapsed.as_millis() as u64) < budget_ms,
        "200 cached renders took {:?} (budget {}ms) — cache regression?",
        elapsed,
        budget_ms,
    );
}
