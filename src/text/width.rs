//! The single source of truth for display-column math.
//!
//! Vim's cursor lives in display columns, not byte offsets or char indices.
//! Tabs expand, CJK characters are double-width, combining marks are
//! zero-width. Every cursor-math function in the editor must go through
//! helpers here — never compute columns ad-hoc.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Width of a single grapheme cluster on screen, accounting for tabs.
///
/// `col` is the display column the grapheme starts at, needed only for tabs
/// (which advance to the next tab stop).
pub fn grapheme_width(g: &str, col: usize, tab_width: usize) -> usize {
    if g == "\t" {
        let stop = tab_width.max(1);
        stop - (col % stop)
    } else {
        UnicodeWidthStr::width(g).max(0)
    }
}

/// Total display width of `s` starting at column 0.
pub fn line_display_width(s: &str, tab_width: usize) -> usize {
    let mut col = 0usize;
    for g in s.graphemes(true) {
        col += grapheme_width(g, col, tab_width);
    }
    col
}

/// Map a display column to a byte offset within `line`.
///
/// If `col` lands inside a wide grapheme or a tab expansion, returns the
/// byte offset of that grapheme (cursor sits on the cell).
/// If `col` is past end-of-line, returns `line.len()`.
pub fn col_to_byte(line: &str, col: usize, tab_width: usize) -> usize {
    let mut cur_col = 0usize;
    let mut last_byte = 0usize;
    for (byte, g) in line.grapheme_indices(true) {
        let w = grapheme_width(g, cur_col, tab_width);
        if cur_col + w > col {
            return byte;
        }
        cur_col += w;
        last_byte = byte + g.len();
    }
    last_byte
}

/// Map a byte offset to a display column within `line`.
pub fn byte_to_col(line: &str, byte: usize, tab_width: usize) -> usize {
    let mut col = 0usize;
    for (b, g) in line.grapheme_indices(true) {
        if b >= byte {
            return col;
        }
        col += grapheme_width(g, col, tab_width);
    }
    col
}

/// Iterate `(byte_start, byte_end, grapheme, start_col, width)` over `line`.
pub fn graphemes_with_cols<'a>(
    line: &'a str,
    tab_width: usize,
) -> impl Iterator<Item = (usize, &'a str, usize, usize)> {
    let mut col = 0usize;
    line.grapheme_indices(true).map(move |(b, g)| {
        let w = grapheme_width(g, col, tab_width);
        let entry = (b, g, col, w);
        col += w;
        entry
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_widths() {
        assert_eq!(line_display_width("hello", 4), 5);
        assert_eq!(col_to_byte("hello", 3, 4), 3);
        assert_eq!(byte_to_col("hello", 3, 4), 3);
    }

    #[test]
    fn tabs_expand_to_next_stop() {
        // tab from col 0 -> 4 (width 4)
        assert_eq!(grapheme_width("\t", 0, 4), 4);
        // tab from col 1 -> 4 (width 3)
        assert_eq!(grapheme_width("\t", 1, 4), 3);
        // tab from col 4 -> 8 (width 4)
        assert_eq!(grapheme_width("\t", 4, 4), 4);
    }

    #[test]
    fn line_with_tab() {
        // "a\tb" with tab_width 4: 'a' at 0 (1), tab fills 1-3 (width 3), 'b' at 4
        assert_eq!(line_display_width("a\tb", 4), 5);
        assert_eq!(col_to_byte("a\tb", 4, 4), 2); // 'b' at byte 2
        assert_eq!(byte_to_col("a\tb", 2, 4), 4);
    }

    #[test]
    fn cjk_is_double_width() {
        assert_eq!(line_display_width("漢字", 4), 4);
        // col 0 -> first char byte 0, col 2 -> second char byte 3
        assert_eq!(col_to_byte("漢字", 0, 4), 0);
        assert_eq!(col_to_byte("漢字", 2, 4), 3);
    }

    #[test]
    fn col_past_end_returns_line_len() {
        assert_eq!(col_to_byte("abc", 99, 4), 3);
    }
}
