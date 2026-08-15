//! M8: `/` `?` `n` `N` search.

use std::io::Write;

use rtdvi::keymap::keys::{Key, KeyCode};
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}
fn press(editor: &mut Editor, code: KeyCode) {
    mode::handle_key(editor, Key::new(code));
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

fn cursor(editor: &Editor) -> (usize, usize) {
    let w = editor.active_window().unwrap();
    (w.cursor.row, w.cursor.col)
}

#[test]
fn slash_jumps_to_first_match() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "/gam");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (2, 0));
}

#[test]
fn incsearch_previews_first_match_while_typing() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    // Type the pattern but do NOT press Enter — the cursor should already jump.
    type_keys(&mut editor, "/gam");
    assert_eq!(cursor(&editor), (2, 0), "incsearch should preview the match");
}

#[test]
fn incsearch_amends_from_origin_when_pattern_changes() {
    let (mut editor, _f) = open("aba\nabc\nabd\n");
    type_keys(&mut editor, "/abc"); // first (only) match is row 1
    assert_eq!(cursor(&editor), (1, 0));
    // Backspace to "ab": re-search from the origin (row 0), not the preview.
    press(&mut editor, KeyCode::Backspace);
    assert_eq!(cursor(&editor), (0, 0), "amended pattern re-searches from origin");
}

#[test]
fn incsearch_esc_restores_origin() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "j"); // origin = row 1
    assert_eq!(cursor(&editor), (1, 0));
    type_keys(&mut editor, "/gam"); // preview jumps to row 2
    assert_eq!(cursor(&editor), (2, 0));
    press(&mut editor, KeyCode::Esc);
    assert_eq!(cursor(&editor), (1, 0), "esc should restore the origin");
}

#[test]
fn incsearch_no_match_stays_at_origin() {
    let (mut editor, _f) = open("alpha\nbeta\n");
    type_keys(&mut editor, "j"); // origin = row 1
    type_keys(&mut editor, "/zzz"); // no match
    assert_eq!(cursor(&editor), (1, 0), "no match keeps the origin position");
}

#[test]
fn search_centres_match_in_view() {
    let content: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (mut editor, _f) = open(&content);
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 20;
    }
    type_keys(&mut editor, "/line 100");
    press(&mut editor, KeyCode::Enter);
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 100);
    // Centered like `{n}G`: top_line = 100 - 20/2 = 90.
    assert_eq!(w.top_line, 90, "search match should be centred, got {}", w.top_line);
}

#[test]
fn incsearch_preview_centres_match() {
    let content: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (mut editor, _f) = open(&content);
    {
        let w_id = editor.tabs[0].active;
        editor.windows.get_mut(&w_id).unwrap().viewport_h = 20;
    }
    type_keys(&mut editor, "/line 100"); // no Enter — live preview
    let w = editor.active_window().unwrap();
    assert_eq!(w.cursor.row, 100);
    assert_eq!(w.top_line, 90, "incsearch preview should centre the match");
}

#[test]
fn incsearch_enter_commits_previewed_match() {
    let (mut editor, _f) = open("alpha\nbeta\ngamma\n");
    type_keys(&mut editor, "/gam");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (2, 0));
    // <C-o> returns to the origin recorded at submit time (row 0).
    mode::handle_key(
        &mut editor,
        Key::with(KeyCode::Char('o'), rtdvi::keymap::keys::KeyMods::CTRL),
    );
    assert_eq!(cursor(&editor), (0, 0), "jumplist should return to the origin");
}

#[test]
fn n_jumps_to_next_match() {
    let (mut editor, _f) = open("foo bar foo bar\n");
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "n");
    assert_eq!(cursor(&editor), (0, 8));
    type_keys(&mut editor, "n");
    // Wraps around.
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn empty_search_repeats_last_pattern() {
    let (mut editor, _f) = open("foo\nbar\nfoo\nbaz\nfoo\n");
    type_keys(&mut editor, "/foo");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
    // Empty `/` + Enter repeats the last pattern forward.
    type_keys(&mut editor, "/");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (2, 0));
    // Empty `?` + Enter repeats it backward.
    type_keys(&mut editor, "?");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn empty_search_repeats_word_search() {
    // The reported flow: a word search (`*`/`#`/`£`) sets the pattern, then
    // an empty `?`/`/` repeats it backward/forward.
    let (mut editor, _f) = open("aa\nfoo\nbb\nfoo\ncc\n");
    type_keys(&mut editor, "j"); // row 1 = first foo
    type_keys(&mut editor, "*"); // \bfoo\b, jumps forward to the next foo
    assert_eq!(cursor(&editor), (3, 0));
    type_keys(&mut editor, "?"); // empty backward repeat
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (1, 0));
    type_keys(&mut editor, "/"); // empty forward repeat
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (3, 0));
}

#[test]
fn question_mark_searches_backward() {
    let (mut editor, _f) = open("aaa bbb ccc\n");
    type_keys(&mut editor, "$"); // end of line
    type_keys(&mut editor, "?aaa");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn capital_n_reverses_direction() {
    let (mut editor, _f) = open("x y x y x\n");
    type_keys(&mut editor, "/x");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (0, 0));
    type_keys(&mut editor, "nn"); // 0 -> 4 -> 8 (last 'x'? Actually 0,4,8 are x)
    assert_eq!(cursor(&editor), (0, 8));
    type_keys(&mut editor, "N"); // back to 4
    assert_eq!(cursor(&editor), (0, 4));
}

#[test]
fn no_match_sets_status_message() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/xyz");
    press(&mut editor, KeyCode::Enter);
    assert!(editor.status_message.as_deref().unwrap_or("").contains("not found"));
}

#[test]
fn esc_cancels_prompt_without_setting_pattern() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/he");
    press(&mut editor, KeyCode::Esc);
    assert!(editor.search.last_pattern.is_none());
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn ctrl_c_cancels_prompt_like_esc() {
    let (mut editor, _f) = open("hello\n");
    type_keys(&mut editor, "/he");
    mode::handle_key(
        &mut editor,
        Key::with(KeyCode::Char('c'), rtdvi::keymap::keys::KeyMods::CTRL),
    );
    assert_eq!(editor.mode, rtdvi::mode::ModeId::Normal);
    assert!(editor.search.prompt.is_empty());
    assert!(editor.search.last_pattern.is_none());
    assert_eq!(cursor(&editor), (0, 0));
}

#[test]
fn backward_search_on_large_buffer_is_fast_and_correct() {
    // ~30k lines with a "8276" match every 1000 lines. Before the O(log n)
    // rope conversions, `?` from end-of-file did O(n²) char-index scans and
    // hung for ~15-20s on a file this size.
    let mut content = String::with_capacity(1_000_000);
    for i in 0..30_000 {
        if i % 1000 == 0 {
            content.push_str(&format!("mark8276 line {i}\n"));
        } else {
            content.push_str(&format!("filler line {i} with some text\n"));
        }
    }
    let (mut editor, _f) = open(&content);
    type_keys(&mut editor, "G"); // jump to end
    type_keys(&mut editor, "?8276");
    press(&mut editor, KeyCode::Enter);
    // The last "8276" before the end-of-file cursor is on line 29000, col 4.
    assert_eq!(cursor(&editor), (29_000, 4));
}

#[test]
fn search_on_mmap_large_file_does_not_crash() {
    // A file larger than the 8 MiB mmap threshold opens with an empty rope
    // (built lazily). Searching used to either find nothing (forward) or panic
    // with "Line index out of bounds" (backward from end). Search now
    // materializes the rope first.
    let mut content = String::with_capacity(13_000_000);
    for i in 0..300_000 {
        if i == 250_000 {
            content.push_str("needle42 here\n");
        } else {
            content.push_str(&format!("filler line {i} padding xxxxxxxxxxxxxxxxxx\n"));
        }
    }
    assert!(content.len() > 8 * 1024 * 1024, "must exceed the mmap threshold");

    let (mut editor, _f) = open(&content);
    // Backward from end-of-file (previously panicked).
    type_keys(&mut editor, "G");
    type_keys(&mut editor, "?needle42");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (250_000, 0));

    // Forward search also works (previously found nothing on the empty rope).
    type_keys(&mut editor, "/needle42");
    press(&mut editor, KeyCode::Enter);
    assert_eq!(cursor(&editor), (250_000, 0));
}
