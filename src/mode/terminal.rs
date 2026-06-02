//! Terminal-job mode: keystrokes are forwarded to the focused terminal's
//! child process, except a `<C-w>` prefix which (as in vim) introduces a
//! window command — `<C-w>c` to close the terminal, `<C-w>h/j/k/l/w` to move
//! focus to another window (dropping back to Normal mode there).

use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::sync_mode_for_active;
use crate::Editor;

pub fn handle_key(editor: &mut Editor, key: Key) {
    if editor.terminal_window_cmd {
        editor.terminal_window_cmd = false;
        handle_window_cmd(editor, key);
        return;
    }
    // `<C-w>` starts a window command; the next key decides what it does.
    if key.code == KeyCode::Char('w') && key.mods == KeyMods::CTRL {
        editor.terminal_window_cmd = true;
        return;
    }
    // Everything else is sent to the job.
    let bytes = key_to_bytes(editor, key);
    if !bytes.is_empty() {
        if let Some(buf_id) = editor.active_buffer_id() {
            if let Some(term) = editor.terminals.get_mut(&buf_id) {
                term.send(&bytes);
            }
        }
    }
}

fn handle_window_cmd(editor: &mut Editor, key: Key) {
    let ctrl = key.mods == KeyMods::CTRL;
    match key.code {
        // Close the terminal (kills the job) — the one binding the user asked for.
        KeyCode::Char('c') => {
            if let Some(buf_id) = editor.active_buffer_id() {
                editor.close_terminal_buffer(buf_id);
            }
        }
        KeyCode::Char('h') => crate::window_actions::focus_dir(editor, 'h'),
        KeyCode::Char('j') => crate::window_actions::focus_dir(editor, 'j'),
        KeyCode::Char('k') => crate::window_actions::focus_dir(editor, 'k'),
        KeyCode::Char('l') => crate::window_actions::focus_dir(editor, 'l'),
        KeyCode::Char('w') => crate::window_actions::focus_dir(editor, 'w'),
        KeyCode::Char('=') if !ctrl => {
            if let Some(tab) = editor.tabs.get_mut(editor.active_tab) {
                tab.tree.equalize();
            }
        }
        _ => {}
    }
    // Whatever happened, the active window may have changed — re-sync the mode.
    sync_mode_for_active(editor);
}

/// Translate a key event into the byte sequence a terminal expects.
fn key_to_bytes(editor: &Editor, key: Key) -> Vec<u8> {
    let app_cursor = editor
        .active_buffer_id()
        .and_then(|id| editor.terminals.get(&id))
        .map(|t| t.parser.lock().map(|p| p.screen().application_cursor()).unwrap_or(false))
        .unwrap_or(false);
    // Cursor / edit keys send CSI (`ESC [`) or, in application-cursor mode,
    // SS3 (`ESC O`) introducer for the arrows.
    let cursor = |c: u8| {
        if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    match key.code {
        KeyCode::Char(c) => {
            if key.mods.contains(KeyMods::CTRL) {
                // Control byte: Ctrl-A = 0x01 … Ctrl-Z = 0x1a, plus the usual
                // punctuation mappings (Ctrl-Space = NUL, etc.).
                let b = ctrl_byte(c);
                let mut out = Vec::new();
                if key.mods.contains(KeyMods::ALT) {
                    out.push(0x1b);
                }
                out.push(b);
                out
            } else {
                let mut out = Vec::new();
                if key.mods.contains(KeyMods::ALT) {
                    out.push(0x1b);
                }
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                out
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],
        KeyCode::Up => cursor(b'A'),
        KeyCode::Down => cursor(b'B'),
        KeyCode::Right => cursor(b'C'),
        KeyCode::Left => cursor(b'D'),
        KeyCode::Home => cursor(b'H'),
        KeyCode::End => cursor(b'F'),
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        KeyCode::F(n) => function_key(n).unwrap_or_default(),
    }
}

/// Map a char to its control byte (the value produced when Ctrl is held).
fn ctrl_byte(c: char) -> u8 {
    let u = c.to_ascii_uppercase();
    match u {
        '@' | ' ' => 0x00,
        'A'..='Z' => (u as u8) - b'A' + 1,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        // Fall back to the raw byte for anything else.
        _ => c as u8,
    }
}

/// xterm-style function-key encodings for F1..F12.
fn function_key(n: u8) -> Option<Vec<u8>> {
    let s: &[u8] = match n {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(s.to_vec())
}
