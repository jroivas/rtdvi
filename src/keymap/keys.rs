//! Key event representation and `"gg"` / `"<C-v>"` string parser.

use crossterm::event::{KeyCode as XCode, KeyEvent as XEvent, KeyModifiers as XMods};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct KeyMods(pub u8);

impl KeyMods {
    pub const NONE: KeyMods = KeyMods(0);
    pub const CTRL: KeyMods = KeyMods(0b001);
    pub const ALT: KeyMods = KeyMods(0b010);
    pub const SHIFT: KeyMods = KeyMods(0b100);

    pub fn contains(self, other: KeyMods) -> bool {
        (self.0 & other.0) == other.0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn union(self, other: KeyMods) -> KeyMods {
        KeyMods(self.0 | other.0)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyMods,
}

impl Key {
    pub fn new(code: KeyCode) -> Self {
        Self { code, mods: KeyMods::NONE }
    }
    pub fn with(code: KeyCode, mods: KeyMods) -> Self {
        Self { code, mods }
    }
    pub fn ctrl(c: char) -> Self {
        Self { code: KeyCode::Char(c.to_ascii_lowercase()), mods: KeyMods::CTRL }
    }
    pub fn char(c: char) -> Self {
        Self { code: KeyCode::Char(c), mods: KeyMods::NONE }
    }
}

/// Convert a crossterm key event to our internal representation.
pub fn from_crossterm(ev: XEvent) -> Option<Key> {
    let mut mods = KeyMods::NONE;
    if ev.modifiers.contains(XMods::CONTROL) {
        mods = mods.union(KeyMods::CTRL);
    }
    if ev.modifiers.contains(XMods::ALT) {
        mods = mods.union(KeyMods::ALT);
    }
    // For Char, SHIFT is captured by the case of the char itself — only flag SHIFT for non-Char codes.
    let code = match ev.code {
        XCode::Char(c) => {
            // Lower-case if CTRL set, so Ctrl-A and Ctrl-a both compare equal.
            if mods.contains(KeyMods::CTRL) {
                KeyCode::Char(c.to_ascii_lowercase())
            } else {
                KeyCode::Char(c)
            }
        }
        XCode::Enter => KeyCode::Enter,
        XCode::Esc => KeyCode::Esc,
        XCode::Backspace => KeyCode::Backspace,
        XCode::Tab => KeyCode::Tab,
        XCode::BackTab => KeyCode::BackTab,
        XCode::Up => KeyCode::Up,
        XCode::Down => KeyCode::Down,
        XCode::Left => KeyCode::Left,
        XCode::Right => KeyCode::Right,
        XCode::Home => KeyCode::Home,
        XCode::End => KeyCode::End,
        XCode::PageUp => KeyCode::PageUp,
        XCode::PageDown => KeyCode::PageDown,
        XCode::Delete => KeyCode::Delete,
        XCode::Insert => KeyCode::Insert,
        XCode::F(n) => KeyCode::F(n),
        _ => return None,
    };
    // Tack SHIFT onto non-Char codes for completeness.
    if !matches!(code, KeyCode::Char(_)) && ev.modifiers.contains(XMods::SHIFT) {
        mods = mods.union(KeyMods::SHIFT);
    }
    Some(Key { code, mods })
}

/// Parse a sequence string like `"gg"` or `"<C-v>j"` into a Vec<Key>.
pub fn parse_sequence(s: &str) -> Result<Vec<Key>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut buf = String::new();
            let mut closed = false;
            for d in chars.by_ref() {
                if d == '>' {
                    closed = true;
                    break;
                }
                buf.push(d);
            }
            if !closed {
                return Err(format!("unterminated '<' in {s:?}"));
            }
            out.push(parse_named(&buf)?);
        } else {
            out.push(Key::char(c));
        }
    }
    Ok(out)
}

fn parse_named(s: &str) -> Result<Key, String> {
    let parts: Vec<&str> = s.split('-').collect();
    let (mods_parts, name) = parts.split_at(parts.len().saturating_sub(1));
    let name = name.first().copied().unwrap_or("");
    let mut mods = KeyMods::NONE;
    for m in mods_parts {
        mods = mods.union(match *m {
            "C" | "c" => KeyMods::CTRL,
            "A" | "a" | "M" | "m" => KeyMods::ALT,
            "S" | "s" => KeyMods::SHIFT,
            _ => return Err(format!("unknown modifier: {m:?}")),
        });
    }
    let code = match name {
        "CR" | "Enter" | "Return" => KeyCode::Enter,
        "Esc" | "ESC" => KeyCode::Esc,
        "BS" | "Backspace" => KeyCode::Backspace,
        "Tab" => KeyCode::Tab,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Del" | "Delete" => KeyCode::Delete,
        "Space" => KeyCode::Char(' '),
        "Bar" => KeyCode::Char('|'),
        "Bslash" => KeyCode::Char('\\'),
        "Lt" => KeyCode::Char('<'),
        "Gt" => KeyCode::Char('>'),
        s if s.starts_with('F') && s[1..].parse::<u8>().is_ok() => {
            KeyCode::F(s[1..].parse().unwrap())
        }
        s if s.chars().count() == 1 => {
            let c = s.chars().next().unwrap();
            // Ctrl-x is stored lowercase for consistency
            if mods.contains(KeyMods::CTRL) {
                KeyCode::Char(c.to_ascii_lowercase())
            } else {
                KeyCode::Char(c)
            }
        }
        other => return Err(format!("unknown key name: {other:?}")),
    };
    Ok(Key { code, mods })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gg() {
        let seq = parse_sequence("gg").unwrap();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq[0], Key::char('g'));
        assert_eq!(seq[1], Key::char('g'));
    }

    #[test]
    fn parse_ctrl_v() {
        let seq = parse_sequence("<C-v>").unwrap();
        assert_eq!(seq.len(), 1);
        assert_eq!(seq[0], Key::ctrl('v'));
    }

    #[test]
    fn parse_mixed() {
        let seq = parse_sequence("<C-w>j").unwrap();
        assert_eq!(seq, vec![Key::ctrl('w'), Key::char('j')]);
    }

    #[test]
    fn parse_esc() {
        let seq = parse_sequence("<Esc>").unwrap();
        assert_eq!(seq, vec![Key::new(KeyCode::Esc)]);
    }

    #[test]
    fn unknown_modifier_rejected() {
        assert!(parse_sequence("<Q-x>").is_err());
    }
}
