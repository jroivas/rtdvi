//! `<C-w>|` maximizes width, `<C-w>_` maximizes height (leaving the cross
//! axis untouched), and `<C-w>T` moves the active window to a new tab page.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode, KeyMods};
use rtdvi::window::WindowId;
use rtdvi::{mode, Editor};
use ratatui::layout::Rect;
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
}
fn ctrl(editor: &mut Editor, c: char) {
    mode::handle_key(editor, Key::with(KeyCode::Char(c), KeyMods::CTRL));
}

fn open(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn layout(editor: &Editor, width: u16, height: u16) -> Vec<(WindowId, Rect)> {
    editor.tabs[editor.active_tab]
        .tree
        .layout(Rect { x: 0, y: 0, width, height })
}

fn active_rect(editor: &Editor, width: u16, height: u16) -> Rect {
    let active = editor.tabs[editor.active_tab].active;
    layout(editor, width, height)
        .into_iter()
        .find(|(w, _)| *w == active)
        .map(|(_, r)| r)
        .unwrap()
}

#[test]
fn maximize_width_grows_active_column() {
    // Two side-by-side columns; the active one is the freshly-split left pane.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "|");

    // Active column should dominate the 100-cell width; the sibling shrinks to
    // the layout floor. (A border column means the active pane lands a little
    // shy of a literal 95.)
    let r = active_rect(&editor, 100, 24);
    let sibling = layout(&editor, 100, 24)
        .into_iter()
        .filter(|(w, _)| *w != editor.tabs[editor.active_tab].active)
        .map(|(_, rc)| rc.width)
        .max()
        .unwrap();
    assert!(r.width >= 85, "active width {} should be near-maximal", r.width);
    assert!(r.width > sibling * 4, "active {} should dwarf sibling {sibling}", r.width);
    assert_eq!(r.height, 24, "height untouched by `<C-w>|`");
}

#[test]
fn maximize_height_grows_active_row() {
    // Two stacked rows; the active one is the freshly-split top pane.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":split");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs[0].tree.windows().len(), 2);

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "_");

    let r = active_rect(&editor, 80, 100);
    assert!(r.height >= 90, "active height {} should be ~95", r.height);
    assert_eq!(r.width, 80, "width untouched by `<C-w>_`");
}

#[test]
fn maximize_height_leaves_column_widths_untouched() {
    // Columns side by side; maximizing height must not disturb their widths.
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);

    let before: Vec<u16> = {
        let mut w: Vec<u16> = layout(&editor, 100, 24).iter().map(|(_, r)| r.width).collect();
        w.sort_unstable();
        w
    };

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "_");

    let after: Vec<u16> = {
        let mut w: Vec<u16> = layout(&editor, 100, 24).iter().map(|(_, r)| r.width).collect();
        w.sort_unstable();
        w
    };
    assert_eq!(before, after, "`<C-w>_` must not change column widths");
}

#[test]
fn move_to_new_tab_peels_off_active_window() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, ":vsplit");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(editor.tabs.len(), 1);
    let moved = editor.tabs[0].active;

    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "T");

    assert_eq!(editor.tabs.len(), 2, "a new tab should be created");
    assert_eq!(editor.active_tab, 1, "focus follows the moved window");
    assert_eq!(editor.tabs[1].tree.windows(), vec![moved]);
    assert_eq!(editor.tabs[1].active, moved);
    // The window left behind keeps the original tab as a single pane.
    assert_eq!(editor.tabs[0].tree.windows().len(), 1);
    assert!(!editor.tabs[0].tree.windows().contains(&moved));
}

#[test]
fn move_to_new_tab_noop_with_single_window() {
    let (mut editor, _f) = open("hello\n");
    ctrl(&mut editor, 'w');
    type_keys(&mut editor, "T");
    assert_eq!(editor.tabs.len(), 1, "lone window stays put");
}
