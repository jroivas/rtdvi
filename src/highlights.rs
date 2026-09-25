//! Persistent text highlights (a.k.a. "marks" or "word highlights").
//!
//! `:highlight <text>` adds a new highlight (or toggles off if `<text>`
//! is already highlighted). `:nohighlight [<text>]` clears one or all.
//! The action [`Highlights::toggle`] is also exposed through
//! `<leader>m` to highlight the word under the cursor.
//!
//! Colours start from a curated palette and then continue with generated
//! hues so the number of concurrent highlights is effectively unbounded.

use ratatui::style::{Color, Style};
use regex::Regex;

/// Curated colour pool. Chosen so each is reasonably distinct and
/// stays readable against both light and dark colorschemes' default
/// terminal backgrounds. Order = age order on first allocation.
pub const PALETTE: &[Color] = &[
    Color::Rgb(255, 215, 0),   // gold
    Color::Rgb(135, 206, 250), // sky blue
    Color::Rgb(255, 105, 180), // hot pink
    Color::Rgb(152, 251, 152), // pale green
    Color::Rgb(255, 165, 0),   // orange
    Color::Rgb(221, 160, 221), // plum
    Color::Rgb(176, 224, 230), // powder blue
    Color::Rgb(255, 250, 205), // lemon chiffon
    Color::Rgb(255, 182, 193), // light pink
    Color::Rgb(173, 216, 230), // light blue
];

#[derive(Debug, Clone)]
pub struct HighlightEntry {
    /// User-facing pattern: literal text from `:highlight <text>` or the
    /// `\bword\b` form for `<leader>m`. Equality on this string drives
    /// toggle / remove.
    pub pattern: String,
    /// Compiled regex of `pattern`. Pre-compiled at insert time so the
    /// renderer doesn't recompile per line.
    pub regex: Regex,
    pub color: Color,
}

impl HighlightEntry {
    pub fn style(&self) -> Style {
        Style::default().bg(self.color).fg(Color::Black)
    }
}

#[derive(Default, Debug)]
pub struct Highlights {
    /// Entries in age order — `entries[0]` is the oldest, `entries.last()`
    /// the newest.
    pub entries: Vec<HighlightEntry>,
}

impl Highlights {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add a literal-string highlight. If `text` is already highlighted
    /// — either as the literal form OR the word-bounded form — that
    /// existing entry is removed instead. Keeps `:hl foo` and `<leader>m`
    /// on `foo` symmetric: either flavour toggles the other off.
    pub fn toggle_literal(&mut self, text: &str) -> ToggleResult {
        self.toggle_literal_with_color(text, None)
    }

    /// Like [`toggle_literal`] but lets the caller pick an explicit colour.
    /// When `color` is `None`, the next auto colour slot is used.
    pub fn toggle_literal_with_color(&mut self, text: &str, color: Option<Color>) -> ToggleResult {
        let lit = regex::escape(text);
        if self.remove_matching(text, &lit) {
            return ToggleResult::Removed(text.to_string());
        }
        self.insert(&lit, text, color)
    }

    /// Toggle a word-bounded highlight: `\bword\b`. Used by the
    /// "highlight word under cursor" action so `foo` doesn't bleed
    /// into `foobar`. Like `toggle_literal`, an existing entry for
    /// the same word (regardless of which form created it) is removed.
    pub fn toggle_word(&mut self, word: &str) -> ToggleResult {
        let word_bounded = format!(r"\b{}\b", regex::escape(word));
        if self.remove_matching(word, &word_bounded) {
            return ToggleResult::Removed(word.to_string());
        }
        self.insert(&word_bounded, word, None)
    }

    /// Look for an existing entry that represents the same word — either
    /// the bare literal form OR the word-bounded form. Returns true and
    /// drops it on a hit. Used by the toggle paths so the two flavours
    /// don't accumulate duplicates for the same logical word.
    fn remove_matching(&mut self, display: &str, current_pattern: &str) -> bool {
        let literal = regex::escape(display);
        let word_bounded = format!(r"\b{}\b", regex::escape(display));
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.pattern == literal || e.pattern == word_bounded || e.pattern == current_pattern)
        {
            self.entries.remove(idx);
            return true;
        }
        false
    }

    fn insert(&mut self, pattern: &str, display: &str, color: Option<Color>) -> ToggleResult {
        let Ok(regex) = Regex::new(pattern) else {
            return ToggleResult::BadPattern;
        };
        let color = color.unwrap_or_else(|| self.next_color());
        self.entries.push(HighlightEntry {
            pattern: pattern.to_string(),
            regex,
            color,
        });
        let _ = display;
        ToggleResult::Added(display.to_string())
    }

    /// Remove a highlight by its display text. Matches either the
    /// literal form OR the word-bounded form for the same text — so
    /// `:nohighlight foo` removes the entry whether it was created by
    /// `:highlight foo` or by `<leader>m` on the word `foo`.
    pub fn remove_literal(&mut self, text: &str) -> bool {
        let literal = regex::escape(text);
        let word_bounded = format!(r"\b{}\b", regex::escape(text));
        self.entries
            .iter()
            .position(|e| e.pattern == literal || e.pattern == word_bounded)
            .map(|i| {
                self.entries.remove(i);
                true
            })
            .unwrap_or(false)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn next_color(&self) -> Color {
        // Reuse the first slot that is not currently in use so active
        // highlights stay distinct even after removals.
        let mut slot = 0usize;
        loop {
            let candidate = color_for_slot(slot);
            if !self.entries.iter().any(|e| e.color == candidate) {
                return candidate;
            }
            slot += 1;
        }
    }
}

/// Parse a user-provided colour for `:highlight`.
///
/// Supports:
/// - `#RRGGBB`
/// - named terminal colours (`red`, `lightblue`, `darkgray`, ...)
/// - numeric xterm index (`0..255`)
pub fn parse_color_spec(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }

    let lower = s.to_ascii_lowercase();
    Some(match lower.as_str() {
        "black" => Color::Black,
        "darkred" => Color::Red,
        "darkgreen" => Color::Green,
        "darkyellow" | "brown" => Color::Yellow,
        "darkblue" => Color::Blue,
        "darkmagenta" => Color::Magenta,
        "darkcyan" => Color::Cyan,
        "lightgray" | "lightgrey" | "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "red" | "lightred" => Color::LightRed,
        "green" | "lightgreen" => Color::LightGreen,
        "yellow" | "lightyellow" => Color::LightYellow,
        "blue" | "lightblue" => Color::LightBlue,
        "magenta" | "lightmagenta" => Color::LightMagenta,
        "cyan" | "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return lower.parse::<u8>().ok().map(Color::Indexed),
    })
}

fn color_for_slot(slot: usize) -> Color {
    if slot < PALETTE.len() {
        return PALETTE[slot];
    }
    let idx = slot - PALETTE.len();
    // Golden-angle hue stepping gives good separation across many colours.
    let hue = (idx as f32 * 137.507_77) % 360.0;
    let (r, g, b) = hsv_to_rgb(hue, 0.6, 0.95);
    Color::Rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hh = h / 60.0;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to_u8 = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r1), to_u8(g1), to_u8(b1))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToggleResult {
    Added(String),
    Removed(String),
    BadPattern,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_then_toggle_off() {
        let mut h = Highlights::new();
        assert!(matches!(h.toggle_literal("foo"), ToggleResult::Added(_)));
        assert_eq!(h.entries.len(), 1);
        assert!(matches!(h.toggle_literal("foo"), ToggleResult::Removed(_)));
        assert!(h.is_empty());
    }

    #[test]
    fn each_add_gets_distinct_color_until_pool_full() {
        let mut h = Highlights::new();
        for i in 0..PALETTE.len() {
            assert!(matches!(
                h.toggle_literal(&format!("t{i}")),
                ToggleResult::Added(_)
            ));
        }
        // All distinct.
        let colors: std::collections::HashSet<_> =
            h.entries.iter().map(|e| format!("{:?}", e.color)).collect();
        assert_eq!(colors.len(), PALETTE.len());
    }

    #[test]
    fn more_than_palette_does_not_drop_oldest() {
        let mut h = Highlights::new();
        for i in 0..(PALETTE.len() + 6) {
            h.toggle_literal(&format!("entry_{i}"));
        }
        assert_eq!(h.entries.len(), PALETTE.len() + 6);
        assert!(h.entries.iter().any(|e| e.pattern == regex::escape("entry_0")));
    }

    #[test]
    fn color_slots_remain_distinct_after_middle_remove() {
        let mut h = Highlights::new();
        h.toggle_literal("a");
        h.toggle_literal("b");
        h.toggle_literal("c");
        assert!(h.remove_literal("b"));
        h.toggle_literal("d");
        let colors: std::collections::HashSet<_> =
            h.entries.iter().map(|e| format!("{:?}", e.color)).collect();
        assert_eq!(colors.len(), h.entries.len());
    }

    #[test]
    fn explicit_color_is_used() {
        let mut h = Highlights::new();
        let c = Color::Rgb(0xfe, 0x00, 0xfe);
        assert!(matches!(
            h.toggle_literal_with_color("foo", Some(c)),
            ToggleResult::Added(_)
        ));
        assert_eq!(h.entries[0].color, c);
    }

    #[test]
    fn word_toggle_uses_word_boundaries() {
        let mut h = Highlights::new();
        h.toggle_word("foo");
        // The compiled regex should NOT match "foobar".
        let re = &h.entries[0].regex;
        assert!(re.is_match("a foo b"));
        assert!(!re.is_match("foobar"));
    }

    #[test]
    fn literal_toggle_uses_substring() {
        let mut h = Highlights::new();
        h.toggle_literal("foo");
        let re = &h.entries[0].regex;
        assert!(re.is_match("foobar"));
        assert!(re.is_match("xxfoox"));
    }

    #[test]
    fn remove_literal_returns_false_if_absent() {
        let mut h = Highlights::new();
        assert!(!h.remove_literal("nothing"));
        h.toggle_literal("foo");
        assert!(h.remove_literal("foo"));
        assert!(!h.remove_literal("foo"));
    }

    /// `:hl foo` then `<leader>m` on `foo` should remove the existing
    /// literal entry, not add a duplicate word-bounded one.
    #[test]
    fn word_toggle_removes_existing_literal_entry() {
        let mut h = Highlights::new();
        assert!(matches!(h.toggle_literal("foo"), ToggleResult::Added(_)));
        assert_eq!(h.entries.len(), 1);
        let result = h.toggle_word("foo");
        assert!(matches!(result, ToggleResult::Removed(_)), "got {result:?}");
        assert!(h.is_empty());
    }

    /// And the reverse: `<leader>m` on `foo` then `:hl foo` should
    /// remove the word-bounded entry.
    #[test]
    fn literal_toggle_removes_existing_word_entry() {
        let mut h = Highlights::new();
        assert!(matches!(h.toggle_word("foo"), ToggleResult::Added(_)));
        assert_eq!(h.entries.len(), 1);
        let result = h.toggle_literal("foo");
        assert!(matches!(result, ToggleResult::Removed(_)), "got {result:?}");
        assert!(h.is_empty());
    }

    #[test]
    fn nohl_works_on_word_bounded_entry_too() {
        let mut h = Highlights::new();
        h.toggle_word("foo");
        // `:nohl foo` should still find and remove it.
        assert!(h.remove_literal("foo"));
        assert!(h.is_empty());
    }

    #[test]
    fn parse_color_spec_accepts_hex_and_named() {
        assert_eq!(parse_color_spec("red"), Some(Color::LightRed));
        assert_eq!(parse_color_spec("#fe00fe"), Some(Color::Rgb(0xfe, 0x00, 0xfe)));
        assert_eq!(parse_color_spec("13"), Some(Color::Indexed(13)));
        assert_eq!(parse_color_spec("#zz00ff"), None);
    }
}
