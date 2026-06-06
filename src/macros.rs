//! Keyboard macros: `q{reg}` … `q` to record, `@{reg}` to play back.
//!
//! Mirrors vim's model exactly: a macro is nothing more than keystrokes
//! stored as *text* in a named register. So a recorded macro can be pasted
//! like any yank, and conversely any yanked text can be executed as keys —
//! yank `itst<Esc>` into register `a`, press `@a`, and it types `tst`.
//!
//! Recording captures each key's textual form (printable chars verbatim,
//! `<Esc>`→`0x1b`, `<CR>`→`\r`, Ctrl-letter→its C0 control byte) into the
//! register. Playback walks the register text back into [`Key`]s and feeds
//! them through [`crate::mode::handle_key`], so a macro replays through the
//! very same dispatch path a human types into.
//!
//! [`pre_dispatch`] is the single hook, called at the top of
//! `mode::handle_key`: it owns the `q`/`@` state machine and the capture of
//! in-flight recordings, returning `true` when it has fully consumed a key.

use crate::keymap::{Key, KeyCode, KeyMods};
use crate::mode::ModeId;
use crate::Editor;

/// Self-recursion guard: a macro that invokes itself (`@a` inside register
/// `a`) bottoms out here instead of looping forever.
const MAX_DEPTH: usize = 100;

/// What the next keystroke means while a `q`/`@` prefix is open.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Await {
    /// No prefix active.
    #[default]
    None,
    /// `q` was pressed; the next key is the record-target register.
    Record,
    /// `@` was pressed; the next key is the register to play (`@` repeats last).
    Play,
}

/// An in-progress recording: the destination register and the keystroke text
/// accumulated so far.
#[derive(Debug, Clone, Default)]
pub struct Recording {
    pub register: char,
    pub keys: String,
}

#[derive(Debug, Default)]
pub struct MacroState {
    pub await_reg: Await,
    pub recording: Option<Recording>,
    /// Re-entrancy depth while feeding a macro back through the dispatcher.
    /// `> 0` means "we are replaying", which suppresses capture so the
    /// expansion of an `@x` inside a recording is not duplicated.
    pub replay_depth: usize,
    /// Last register played, for `@@`.
    pub last_played: Option<char>,
}

impl MacroState {
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }
}

/// Top-of-dispatch hook. Returns `true` when the key was consumed as a macro
/// control key (and must not reach mode dispatch). Otherwise captures the key
/// into any active recording and returns `false` so normal dispatch proceeds.
pub fn pre_dispatch(editor: &mut Editor, key: Key) -> bool {
    // 1. Resolve a register letter awaited after `q` / `@`.
    match editor.macros.await_reg {
        Await::Record => {
            editor.macros.await_reg = Await::None;
            match register_letter(key) {
                Some(reg) => {
                    editor.macros.recording = Some(Recording { register: reg, keys: String::new() });
                    editor.status_message = Some(format!("recording @{reg}"));
                }
                None => editor.status_message = Some("q: not a valid register".into()),
            }
            return true; // the register letter is never recorded nor dispatched
        }
        Await::Play => {
            editor.macros.await_reg = Await::None;
            let reg = match key.code {
                // `@@` replays the most recent macro.
                KeyCode::Char('@') => editor.macros.last_played,
                _ => register_letter(key),
            };
            // `@x` is itself part of an enclosing recording; capture it before
            // running the playback (whose expansion is suppressed by depth).
            capture(editor, key);
            match reg {
                Some(reg) => play(editor, reg),
                None => editor.status_message = Some("@: not a valid register".into()),
            }
            return true;
        }
        Await::None => {}
    }

    // 2. Bare `q` / `@`, only in Normal mode at a clean dispatch point, so they
    //    stay literal inside insert text and operator-pending sequences. The
    //    `"`-register prefix is also resolved later in `normal::handle_key`, so
    //    skip while one is pending — otherwise `"qyy` (system-clipboard yank)
    //    would mistake its `q` register letter for a record toggle.
    if editor.mode == ModeId::Normal
        && editor.pending_keys.is_empty()
        && key.mods.is_empty()
        && matches!(editor.registers.pending, crate::registers::Pending::None)
    {
        match key.code {
            KeyCode::Char('q') => {
                if editor.macros.is_recording() {
                    stop_recording(editor); // the closing `q` is not captured
                } else {
                    editor.macros.await_reg = Await::Record;
                }
                return true;
            }
            KeyCode::Char('@') => {
                capture(editor, key); // `@` is part of an enclosing recording
                editor.macros.await_reg = Await::Play;
                return true;
            }
            _ => {}
        }
    }

    // 3. Ordinary key: fold it into any recording, then let dispatch run.
    capture(editor, key);
    false
}

/// Append `key` to the active recording, unless we are mid-playback (the
/// expansion of an `@x` must not be re-recorded on top of the literal `@x`).
fn capture(editor: &mut Editor, key: Key) {
    if editor.macros.replay_depth > 0 {
        return;
    }
    if let Some(text) = key_to_text(key) {
        if let Some(rec) = editor.macros.recording.as_mut() {
            rec.keys.push_str(&text);
        }
    }
}

fn stop_recording(editor: &mut Editor) {
    if let Some(rec) = editor.macros.recording.take() {
        editor.named_registers.insert(
            rec.register,
            crate::editor::Register { text: rec.keys, linewise: false },
        );
        editor.status_message = Some(format!("recorded @{}", rec.register));
    }
}

/// Play register `reg` `count` times by feeding its text back as keystrokes.
/// Honours a pending count (`3@q`), so the caller need not.
pub fn play(editor: &mut Editor, reg: char) {
    let reg = reg.to_ascii_lowercase();
    let Some(text) = editor.named_registers.get(&reg).map(|r| r.text.clone()) else {
        editor.status_message = Some(format!("@{reg}: register empty"));
        return;
    };
    editor.macros.last_played = Some(reg);

    if editor.macros.replay_depth >= MAX_DEPTH {
        editor.status_message = Some("macro: recursion limit reached".into());
        return;
    }

    let count = editor.take_count();
    let keys = text_to_keys(&text);
    editor.macros.replay_depth += 1;
    for _ in 0..count {
        for key in &keys {
            crate::mode::handle_key(editor, *key);
        }
    }
    editor.macros.replay_depth = editor.macros.replay_depth.saturating_sub(1);
}

/// A valid macro register: `a`..`z` / `0`..`9` with no Ctrl/Alt. Folded to
/// lowercase so the storage key matches the yank/paste register namespace.
fn register_letter(key: Key) -> Option<char> {
    if let KeyCode::Char(c) = key.code {
        if c.is_ascii_alphanumeric() && !key.mods.contains(KeyMods::CTRL) && !key.mods.contains(KeyMods::ALT) {
            return Some(c.to_ascii_lowercase());
        }
    }
    None
}

/// Serialize one keystroke to register text. Returns `None` for keys with no
/// compact textual form (arrows, function keys), which are dropped from the
/// macro rather than mis-encoded.
fn key_to_text(key: Key) -> Option<String> {
    match key.code {
        KeyCode::Char(c) => {
            if key.mods.contains(KeyMods::CTRL) {
                // Ctrl-letter → its C0 control byte (Ctrl-a = 0x01).
                let b = c.to_ascii_lowercase() as u8;
                if b.is_ascii_lowercase() {
                    Some(((b - b'a' + 1) as char).to_string())
                } else {
                    None
                }
            } else {
                Some(c.to_string())
            }
        }
        KeyCode::Enter => Some('\r'.to_string()),
        KeyCode::Esc => Some('\u{1b}'.to_string()),
        KeyCode::Tab => Some('\t'.to_string()),
        KeyCode::Backspace => Some('\u{8}'.to_string()),
        _ => None,
    }
}

/// Parse register text back into keystrokes — the inverse of [`key_to_text`]
/// over its representable set, and a best-effort reading of arbitrary text
/// (so a yanked snippet executes sensibly).
fn text_to_keys(s: &str) -> Vec<Key> {
    s.chars().map(char_to_key).collect()
}

fn char_to_key(c: char) -> Key {
    match c {
        '\u{1b}' => Key::new(KeyCode::Esc),
        '\r' | '\n' => Key::new(KeyCode::Enter),
        '\t' => Key::new(KeyCode::Tab),
        '\u{8}' | '\u{7f}' => Key::new(KeyCode::Backspace),
        // Remaining C0 controls (Ctrl-a..Ctrl-z) — the named ones above were
        // already peeled off, so what's left maps straight to a Ctrl-letter.
        c if ('\u{1}'..='\u{1a}').contains(&c) => {
            let letter = (b'a' + (c as u8 - 1)) as char;
            Key::with(KeyCode::Char(letter), KeyMods::CTRL)
        }
        c => Key::char(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(key: Key) -> Key {
        let text = key_to_text(key).expect("representable");
        let keys = text_to_keys(&text);
        assert_eq!(keys.len(), 1);
        keys[0]
    }

    #[test]
    fn printable_roundtrips() {
        assert_eq!(roundtrip(Key::char('i')), Key::char('i'));
        assert_eq!(roundtrip(Key::char('A')), Key::char('A'));
        assert_eq!(roundtrip(Key::char('|')), Key::char('|'));
    }

    #[test]
    fn special_keys_roundtrip() {
        assert_eq!(roundtrip(Key::new(KeyCode::Esc)), Key::new(KeyCode::Esc));
        assert_eq!(roundtrip(Key::new(KeyCode::Enter)), Key::new(KeyCode::Enter));
        assert_eq!(roundtrip(Key::new(KeyCode::Tab)), Key::new(KeyCode::Tab));
    }

    #[test]
    fn ctrl_letter_roundtrips() {
        let cw = Key::with(KeyCode::Char('w'), KeyMods::CTRL);
        assert_eq!(roundtrip(cw), cw);
        // Encoded as the single 0x17 control byte.
        assert_eq!(key_to_text(cw).as_deref(), Some("\u{17}"));
    }

    #[test]
    fn plain_text_parses_as_typed_keys() {
        // The user's example: "itst" yanked into a register and played.
        let keys = text_to_keys("itst");
        assert_eq!(keys, vec![
            Key::char('i'),
            Key::char('t'),
            Key::char('s'),
            Key::char('t'),
        ]);
    }

    #[test]
    fn arrow_keys_are_dropped() {
        assert_eq!(key_to_text(Key::new(KeyCode::Up)), None);
    }
}
