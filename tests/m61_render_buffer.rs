//! Render buffers: non-editable, styled content opened via the plugin render
//! pipeline (`PendingAction::OpenRenderBuffer`). Covers read-only enforcement,
//! link navigation keys, and that rendering works through `ui::render`.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::mode::{self, ModeId};
use rtdvi::plugin::pending::{apply_pending, RenderSpanSpec};
use rtdvi::plugin::PendingAction;
use rtdvi::{ui, Editor};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tempfile::NamedTempFile;

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::with_suffix(".md").unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn span(text: &str) -> RenderSpanSpec {
    RenderSpanSpec { text: text.into(), ..Default::default() }
}

/// Open a render buffer with two lines; the 2nd has an external link "docs"
/// at columns 4..8.
fn open_render(editor: &mut Editor) {
    let lines = vec![
        vec![RenderSpanSpec { text: "Title".into(), bold: true, size: 1, ..Default::default() }],
        vec![
            span("see "),
            RenderSpanSpec { text: "docs".into(), link: Some("http://example.com".into()), underline: true, ..Default::default() },
            span(" end"),
        ],
    ];
    apply_pending(
        editor,
        vec![PendingAction::OpenRenderBuffer { title: "[Markdown]".into(), producer: Some("md".into()), lines }],
        "test",
    );
}

fn active_is_render(editor: &Editor) -> bool {
    let id = editor.active_buffer_id().unwrap();
    !editor.buffers.get(&id).unwrap().is_editable()
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn open_render_buffer_makes_active_window_non_editable() {
    let (mut editor, _f) = open("# Title\nsee [docs](x)\n");
    open_render(&mut editor);
    assert!(active_is_render(&editor), "active window should show the render buffer");
    // The source markdown buffer is still around in another window.
    assert!(editor.windows.len() >= 2);
}

#[test]
fn render_buffer_rejects_insert() {
    let (mut editor, _f) = open("# Title\n");
    open_render(&mut editor);
    mode::switch_mode(&mut editor, ModeId::Insert);
    assert_eq!(editor.mode, ModeId::Normal, "must not enter insert on a render buffer");
    assert!(editor
        .status_message
        .as_deref()
        .unwrap_or("")
        .contains("not modifiable"));
}

#[test]
fn tab_jumps_to_link_and_enter_follows() {
    let (mut editor, _f) = open("# Title\n");
    open_render(&mut editor);
    // <Tab> moves the cursor onto the first link (row 1, col 4).
    mode::handle_key(&mut editor, Key::new(KeyCode::Tab));
    assert_eq!(cursor(&editor), (1, 4));
    // <Enter> on an external (http) link reports it rather than navigating.
    mode::handle_key(&mut editor, Key::new(KeyCode::Enter));
    assert!(editor
        .status_message
        .as_deref()
        .unwrap_or("")
        .contains("external link"));
}

#[test]
fn motions_still_scroll_a_render_buffer() {
    let (mut editor, _f) = open("# Title\n");
    open_render(&mut editor);
    // `j` is not claimed by the render-buffer handler → normal motion runs.
    mode::handle_key(&mut editor, Key::char('j'));
    assert_eq!(cursor(&editor).0, 1, "j should move down one line");
}

/// Draw once so the viewport size + scroll are updated (as in the real loop).
fn render_once(editor: &mut Editor, w: u16, h: u16) {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(editor, f)).unwrap();
}

fn open_tall_render(editor: &mut Editor, n: usize) {
    let lines: Vec<_> = (0..n).map(|i| vec![span(&format!("line {i}"))]).collect();
    apply_pending(
        editor,
        vec![PendingAction::OpenRenderBuffer { title: "[md]".into(), producer: None, lines }],
        "test",
    );
}

#[test]
fn j_k_scroll_vertically_like_a_normal_buffer() {
    let (mut editor, _f) = open("# Title\n");
    open_tall_render(&mut editor, 60);
    render_once(&mut editor, 40, 12); // establish viewport height

    for _ in 0..40 {
        mode::handle_key(&mut editor, Key::char('j'));
    }
    render_once(&mut editor, 40, 12);
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 40, "j moves the cursor down");
    assert!(w.top_line > 0, "the view scrolled to keep the cursor visible");

    // k scrolls back up to the top.
    for _ in 0..40 {
        mode::handle_key(&mut editor, Key::char('k'));
    }
    render_once(&mut editor, 40, 12);
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 0);
    assert_eq!(w.top_line, 0);
}

#[test]
fn gg_and_capital_g_jump_in_render_buffer() {
    let (mut editor, _f) = open("# Title\n");
    open_tall_render(&mut editor, 60);
    render_once(&mut editor, 40, 12);
    mode::handle_key(&mut editor, Key::char('G'));
    render_once(&mut editor, 40, 12);
    assert_eq!(editor.active_window().unwrap().cursor.row, 59, "G goes to last line");
}

#[test]
fn l_scrolls_horizontally() {
    let (mut editor, _f) = open("# Title\n");
    let wide = "x".repeat(200);
    apply_pending(
        &mut editor,
        vec![PendingAction::OpenRenderBuffer { title: "[md]".into(), producer: None, lines: vec![vec![span(&wide)]] }],
        "test",
    );
    render_once(&mut editor, 30, 10);
    for _ in 0..50 {
        mode::handle_key(&mut editor, Key::char('l'));
    }
    render_once(&mut editor, 30, 10);
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.col, 50);
    assert!(w.left_col > 0, "the view scrolled right to follow the cursor");
}

#[test]
fn renders_through_ui_without_panicking() {
    let (mut editor, _f) = open("# Title\n");
    open_render(&mut editor);
    let backend = TestBackend::new(40, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text: String = (0..buf.area().height)
        .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_string())
        .collect();
    assert!(text.contains("Title"), "rendered frame should contain the styled content");
    assert!(text.contains('~'), "rows past the render content show the ~ filler");
}

fn open_two_lines(editor: &mut Editor) {
    let lines = vec![vec![span("hello world")], vec![span("second line")]];
    apply_pending(
        editor,
        vec![PendingAction::OpenRenderBuffer { title: "[md]".into(), producer: None, lines }],
        "test",
    );
}

#[test]
fn non_modifying_motions_work() {
    let (mut editor, _f) = open("# Title\n");
    open_two_lines(&mut editor);
    // `$` to end of line, `0` to start, `w` to next word, `G`/`gg`.
    mode::handle_key(&mut editor, Key::char('$'));
    assert_eq!(cursor(&editor), (0, 10)); // end of "hello world"
    mode::handle_key(&mut editor, Key::char('0'));
    assert_eq!(cursor(&editor), (0, 0));
    mode::handle_key(&mut editor, Key::char('w'));
    assert_eq!(cursor(&editor), (0, 6)); // start of "world"
    mode::handle_key(&mut editor, Key::char('G'));
    assert_eq!(cursor(&editor).0, 1);
}

#[test]
fn visual_select_and_yank_work_but_not_paste() {
    let (mut editor, _f) = open("# Title\n");
    open_two_lines(&mut editor);
    // Visually select the whole first line and yank it.
    mode::handle_key(&mut editor, Key::char('v'));
    assert_eq!(editor.mode, ModeId::Visual);
    mode::handle_key(&mut editor, Key::char('$'));
    mode::handle_key(&mut editor, Key::char('y'));
    assert_eq!(editor.mode, ModeId::Normal);
    assert_eq!(editor.unnamed_register.text, "hello world");

    // Paste is a no-op — the buffer is read-only and stays unchanged.
    let id = editor.active_buffer_id().unwrap();
    let before = editor.buffers.get(&id).unwrap().rope().to_string();
    mode::handle_key(&mut editor, Key::char('p'));
    let after = editor.buffers.get(&id).unwrap().rope().to_string();
    assert_eq!(before, after, "paste must not modify a render buffer");
}

#[test]
fn visual_selection_is_painted() {
    let (mut editor, _f) = open("# Title\n");
    open_two_lines(&mut editor);
    render_once(&mut editor, 40, 12);
    // Select the first line characterwise.
    mode::handle_key(&mut editor, Key::char('v'));
    mode::handle_key(&mut editor, Key::char('$'));

    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(&mut editor, f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    // The first selected cell carries the selection background.
    assert_eq!(buf[(0u16, 0u16)].bg, ratatui::style::Color::Rgb(60, 80, 110));
}

#[test]
fn dollar_lands_at_end_of_every_line() {
    let (mut editor, _f) = open("# T\n");
    let texts = [
        "# h1 Heading 8-)",
        "## h2 Heading",
        "##### h5 Heading",
        "###### h6 Heading",
        "",
        "Alternatively, for H1 and H2:",
        "Alt-H1",
        "======",
    ];
    let lines: Vec<_> = texts.iter().map(|t| vec![span(t)]).collect();
    apply_pending(&mut editor, vec![PendingAction::OpenRenderBuffer { title: "[md]".into(), producer: None, lines }], "test");
    render_once(&mut editor, 80, 40);

    for (i, t) in texts.iter().enumerate() {
        // Jump to top, then down to line i.
        mode::handle_key(&mut editor, Key::char('g'));
        mode::handle_key(&mut editor, Key::char('g'));
        for _ in 0..i {
            mode::handle_key(&mut editor, Key::char('j'));
        }
        let row = editor.active_window().unwrap().cursor.row;
        assert_eq!(row, i, "after gg+{i}j cursor should be on row {i}, got {row}");
        mode::handle_key(&mut editor, Key::char('$'));
        let col = editor.active_window().unwrap().cursor.col;
        let expect = t.chars().count().saturating_sub(1);
        assert_eq!(col, expect, "line {i} {t:?}: $ -> col {col}, expected {expect}");
    }
}
