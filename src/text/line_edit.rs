//! Terminal-agnostic single-line editing shared by the `:` command line and
//! the `/`/`?` search prompt.
//!
//! Both prompts otherwise rely on the terminal emitting Home/End/Delete and
//! word-motion keys. Many macOS terminals don't send those by default (Home/End
//! need Fn+arrows, forward-Delete needs Fn+Delete, Option word-motion needs
//! "Use Option as Meta"), so the editing experience there lagged Linux. These
//! emacs/readline-style bindings send plain control bytes every terminal
//! delivers, giving both prompts identical behaviour across platforms.
//!
//! Everything operates on `(text, cursor)` where `cursor` is a byte index that
//! always sits on a char boundary. Delete helpers return `true` when they
//! actually changed the text so callers can refresh completion / search preview.

use crate::keymap::{Key, KeyCode, KeyMods};

/// Try to handle `key` as a readline-style editing key against `(text, cursor)`.
///
/// Returns `Some(changed)` when the key was one this handler owns — `changed`
/// is whether the text was modified (vs a pure cursor move). Returns `None`
/// for keys the prompt must handle itself: Enter, Esc, Up/Down, Ctrl-C, plain
/// Backspace (its empty-line behaviour is prompt-specific), and printable
/// character insertion.
pub fn handle_edit_key(key: Key, text: &mut String, cursor: &mut usize) -> Option<bool> {
    let ctrl = key.mods.contains(KeyMods::CTRL);
    let alt = key.mods.contains(KeyMods::ALT);
    match key.code {
        // Word-wise motion: Ctrl/Alt + arrows, plus Alt-b / Alt-f.
        KeyCode::Left if ctrl || alt => {
            move_word_left(text, cursor);
            Some(false)
        }
        KeyCode::Right if ctrl || alt => {
            move_word_right(text, cursor);
            Some(false)
        }
        KeyCode::Char('b') if alt => {
            move_word_left(text, cursor);
            Some(false)
        }
        KeyCode::Char('f') if alt => {
            move_word_right(text, cursor);
            Some(false)
        }
        // Char-wise motion.
        KeyCode::Left => {
            move_left(text, cursor);
            Some(false)
        }
        KeyCode::Right => {
            move_right(text, cursor);
            Some(false)
        }
        KeyCode::Home => {
            *cursor = 0;
            Some(false)
        }
        KeyCode::End => {
            *cursor = text.len();
            Some(false)
        }
        KeyCode::Char('a') if ctrl => {
            *cursor = 0;
            Some(false)
        }
        KeyCode::Char('e') if ctrl => {
            *cursor = text.len();
            Some(false)
        }
        KeyCode::Char('b') if ctrl => {
            move_left(text, cursor);
            Some(false)
        }
        KeyCode::Char('f') if ctrl => {
            move_right(text, cursor);
            Some(false)
        }
        // Deletion.
        KeyCode::Delete => Some(delete_forward(text, *cursor)),
        KeyCode::Char('d') if ctrl => Some(delete_forward(text, *cursor)),
        KeyCode::Char('d') if alt => Some(delete_word_forward(text, *cursor)),
        KeyCode::Char('u') if ctrl => Some(delete_to_start(text, cursor)),
        KeyCode::Char('k') if ctrl => Some(delete_to_end(text, *cursor)),
        KeyCode::Char('w') if ctrl => Some(delete_word_back(text, cursor)),
        // Alt-Backspace also deletes the previous word (common readline binding).
        KeyCode::Backspace if alt || ctrl => Some(delete_word_back(text, cursor)),
        _ => None,
    }
}

/// Ctrl-B / Left: move one char left.
pub fn move_left(text: &str, cursor: &mut usize) {
    if let Some((i, _)) = text[..*cursor].char_indices().next_back() {
        *cursor = i;
    }
}

/// Ctrl-F / Right: move one char right.
pub fn move_right(text: &str, cursor: &mut usize) {
    if *cursor < text.len() {
        let ch = text[*cursor..].chars().next().unwrap();
        *cursor += ch.len_utf8();
    }
}

/// Alt-B / Ctrl-Left: move to the start of the previous whitespace-delimited word.
pub fn move_word_left(text: &str, cursor: &mut usize) {
    *cursor = word_back_boundary(text, *cursor);
}

/// Alt-F / Ctrl-Right: move past the end of the next whitespace-delimited word.
pub fn move_word_right(text: &str, cursor: &mut usize) {
    *cursor = word_forward_boundary(text, *cursor);
}

/// Backspace: delete the char before the cursor. Returns whether it changed.
pub fn delete_back(text: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 {
        return false;
    }
    let prev = text[..*cursor]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0);
    text.replace_range(prev..*cursor, "");
    *cursor = prev;
    true
}

/// Delete / Ctrl-D: delete the char at the cursor.
pub fn delete_forward(text: &mut String, cursor: usize) -> bool {
    if cursor >= text.len() {
        return false;
    }
    let ch = text[cursor..].chars().next().unwrap();
    text.replace_range(cursor..cursor + ch.len_utf8(), "");
    true
}

/// Ctrl-U: delete from the cursor to the start of the line.
pub fn delete_to_start(text: &mut String, cursor: &mut usize) -> bool {
    if *cursor == 0 {
        return false;
    }
    text.replace_range(0..*cursor, "");
    *cursor = 0;
    true
}

/// Ctrl-K: delete from the cursor to the end of the line.
pub fn delete_to_end(text: &mut String, cursor: usize) -> bool {
    if cursor >= text.len() {
        return false;
    }
    text.truncate(cursor);
    true
}

/// Ctrl-W: delete the whitespace-delimited word before the cursor.
pub fn delete_word_back(text: &mut String, cursor: &mut usize) -> bool {
    let start = word_back_boundary(text, *cursor);
    if start == *cursor {
        return false;
    }
    text.replace_range(start..*cursor, "");
    *cursor = start;
    true
}

/// Alt-D: delete the whitespace-delimited word after the cursor.
pub fn delete_word_forward(text: &mut String, cursor: usize) -> bool {
    let end = word_forward_boundary(text, cursor);
    if end == cursor {
        return false;
    }
    text.replace_range(cursor..end, "");
    true
}

/// Byte index of the start of the word before `cursor`: skip any run of
/// whitespace, then the run of non-whitespace before it.
fn word_back_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    let mut iter = text[..cursor].char_indices().rev().peekable();
    while let Some(&(i, c)) = iter.peek() {
        if c.is_whitespace() {
            idx = i;
            iter.next();
        } else {
            break;
        }
    }
    while let Some(&(i, c)) = iter.peek() {
        if !c.is_whitespace() {
            idx = i;
            iter.next();
        } else {
            break;
        }
    }
    idx
}

/// Byte index just past the end of the word after `cursor`: skip any leading
/// whitespace, then the run of non-whitespace.
fn word_forward_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    let mut iter = text[cursor..].char_indices().peekable();
    while let Some(&(off, c)) = iter.peek() {
        if c.is_whitespace() {
            idx = cursor + off + c.len_utf8();
            iter.next();
        } else {
            break;
        }
    }
    while let Some(&(off, c)) = iter.peek() {
        if !c.is_whitespace() {
            idx = cursor + off + c.len_utf8();
            iter.next();
        } else {
            break;
        }
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyMods) -> Key {
        Key::with(code, mods)
    }

    #[test]
    fn ctrl_a_and_e_jump_to_ends() {
        let mut s = "hello world".to_string();
        let mut c = 4;
        assert_eq!(handle_edit_key(key(KeyCode::Char('a'), KeyMods::CTRL), &mut s, &mut c), Some(false));
        assert_eq!(c, 0);
        assert_eq!(handle_edit_key(key(KeyCode::Char('e'), KeyMods::CTRL), &mut s, &mut c), Some(false));
        assert_eq!(c, s.len());
    }

    #[test]
    fn ctrl_u_deletes_to_start() {
        let mut s = "foo bar".to_string();
        let mut c = 4;
        assert_eq!(handle_edit_key(key(KeyCode::Char('u'), KeyMods::CTRL), &mut s, &mut c), Some(true));
        assert_eq!(s, "bar");
        assert_eq!(c, 0);
    }

    #[test]
    fn ctrl_k_deletes_to_end() {
        let mut s = "foo bar".to_string();
        let mut c = 3;
        assert_eq!(handle_edit_key(key(KeyCode::Char('k'), KeyMods::CTRL), &mut s, &mut c), Some(true));
        assert_eq!(s, "foo");
    }

    #[test]
    fn ctrl_w_deletes_previous_word() {
        let mut s = "edit /some/path/file".to_string();
        let mut c = s.len();
        assert_eq!(handle_edit_key(key(KeyCode::Char('w'), KeyMods::CTRL), &mut s, &mut c), Some(true));
        assert_eq!(s, "edit ");
        assert_eq!(c, 5);
    }

    #[test]
    fn ctrl_w_at_start_is_noop() {
        let mut s = "abc".to_string();
        let mut c = 0;
        assert_eq!(handle_edit_key(key(KeyCode::Char('w'), KeyMods::CTRL), &mut s, &mut c), Some(false));
        assert_eq!(s, "abc");
    }

    #[test]
    fn ctrl_d_deletes_char_under_cursor() {
        let mut s = "abc".to_string();
        let mut c = 1;
        assert_eq!(handle_edit_key(key(KeyCode::Char('d'), KeyMods::CTRL), &mut s, &mut c), Some(true));
        assert_eq!(s, "ac");
        assert_eq!(c, 1);
    }

    #[test]
    fn alt_word_motion_and_delete() {
        let mut s = "foo bar baz".to_string();
        let mut c = s.len();
        // Alt-b twice → start of "bar".
        handle_edit_key(key(KeyCode::Char('b'), KeyMods::ALT), &mut s, &mut c);
        assert_eq!(&s[c..], "baz");
        handle_edit_key(key(KeyCode::Char('b'), KeyMods::ALT), &mut s, &mut c);
        assert_eq!(&s[c..], "bar baz");
        // Alt-d deletes "bar".
        assert_eq!(handle_edit_key(key(KeyCode::Char('d'), KeyMods::ALT), &mut s, &mut c), Some(true));
        assert_eq!(s, "foo  baz");
    }

    #[test]
    fn ctrl_c_and_plain_chars_are_not_handled() {
        let mut s = "x".to_string();
        let mut c = 1;
        assert_eq!(handle_edit_key(key(KeyCode::Char('c'), KeyMods::CTRL), &mut s, &mut c), None);
        assert_eq!(handle_edit_key(Key::new(KeyCode::Char('z')), &mut s, &mut c), None);
        assert_eq!(handle_edit_key(Key::new(KeyCode::Enter), &mut s, &mut c), None);
    }
}
