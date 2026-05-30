//! `h` / `l` jump over tab characters in a single step. A tab displays
//! as several cells but counts as one grapheme — vim parity for cursor
//! motion. Wide CJK characters get the same treatment.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

fn open_with(content: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

// ---- Tabs at line start ---------------------------------------------------

#[test]
fn l_jumps_over_leading_tab() {
    // "\tdata" displays as "    data" with tab_width=4. From col 0
    // (cursor on the tab cell) one `l` should land on 'd' at col 4.
    let (mut editor, _f) = open_with("\tdata\n");
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "l");
    assert_eq!(cursor(&editor), (0, 4));
}

#[test]
fn l_advances_one_char_after_tab() {
    let (mut editor, _f) = open_with("\tdata\n");
    type_keys(&mut editor, "l"); // col 4 (on 'd')
    type_keys(&mut editor, "l"); // col 5 (on 'a')
    assert_eq!(cursor(&editor), (0, 5));
}

#[test]
fn h_jumps_back_over_tab() {
    let (mut editor, _f) = open_with("\tdata\n");
    type_keys(&mut editor, "l"); // col 4 (on 'd')
    type_keys(&mut editor, "h"); // back over the tab in one press
    assert_eq!(cursor(&editor), (0, 0));
}

// ---- Tab in the middle of a line -----------------------------------------

#[test]
fn l_jumps_over_mid_line_tab() {
    // "    - some\tother" with tab_width=4:
    //   '    - some' = cols 0..9
    //   '\t' starts at col 10, fills to col 12 (next stop), width 2
    //   'other' starts at col 12
    let (mut editor, _f) = open_with("    - some\tother\n");
    // Walk the cursor to col 10 (start of the tab).
    for _ in 0..10 {
        type_keys(&mut editor, "l");
    }
    assert_eq!(cursor(&editor), (0, 10));
    // One more `l` should land on 'o' at col 12, NOT col 11.
    type_keys(&mut editor, "l");
    assert_eq!(cursor(&editor), (0, 12));
}

#[test]
fn h_back_over_mid_line_tab() {
    let (mut editor, _f) = open_with("    - some\tother\n");
    for _ in 0..11 {
        type_keys(&mut editor, "l");
    }
    // Should now be on 'o' of "other" at col 12.
    assert_eq!(cursor(&editor), (0, 12));
    type_keys(&mut editor, "h");
    assert_eq!(cursor(&editor), (0, 10));
}

// ---- Counts compose ------------------------------------------------------

#[test]
fn count_l_jumps_per_grapheme() {
    // Two tabs back to back: "\t\tx" → cols 0 and 4 are tab starts, 'x'
    // at col 8. `2l` should land on 'x'.
    let (mut editor, _f) = open_with("\t\tx\n");
    type_keys(&mut editor, "2l");
    assert_eq!(cursor(&editor), (0, 8));
}

// ---- Mid-tab cursor (landed via sticky-col) ------------------------------

#[test]
fn l_from_mid_tab_snaps_to_next_grapheme() {
    // Land cursor mid-tab via vertical motion: previous line is wider
    // than the tab so sticky_col puts us at col 2 of the tab cells.
    let (mut editor, _f) = open_with("abcdef\n\tdata\n");
    type_keys(&mut editor, "ll"); // col 2 on first line
    type_keys(&mut editor, "j");  // line 2; sticky lands at col 2 (mid-tab)
    assert_eq!(cursor(&editor), (1, 2));
    type_keys(&mut editor, "l");
    // Should snap forward to col 4 (start of 'd'), not col 3.
    assert_eq!(cursor(&editor), (1, 4));
}

#[test]
fn h_from_mid_tab_snaps_to_tab_start() {
    let (mut editor, _f) = open_with("abcdef\n\tdata\n");
    type_keys(&mut editor, "lll"); // col 3 on first line
    type_keys(&mut editor, "j");   // mid-tab
    assert_eq!(cursor(&editor), (1, 3));
    type_keys(&mut editor, "h");
    // Snap back to col 0 (start of the tab).
    assert_eq!(cursor(&editor), (1, 0));
}

// ---- CJK gets the same treatment ----------------------------------------

#[test]
fn l_jumps_over_wide_cjk_char() {
    // "漢字" — each char is 2 cells wide. From col 0 one `l` should
    // land on '字' at col 2.
    let (mut editor, _f) = open_with("漢字\n");
    type_keys(&mut editor, "l");
    assert_eq!(cursor(&editor), (0, 2));
}
