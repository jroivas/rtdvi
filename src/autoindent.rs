//! Automatic indentation helpers used by insert mode.
//!
//! Rules are filetype-specific and implemented as a built-in layer —
//! the same philosophy as the keyword lists in `syntax.rs`. No external
//! files are required; a vim runtime installation does not affect indent.
//!
//! Two behaviours are provided:
//!
//! * **`next_line_indent`** — called on Enter / `o`: copy the current
//!   line's leading whitespace and, when the cursor is at end-of-line,
//!   add one extra level for language constructs that introduce a block.
//!
//! * **`brace_dedent`** — called when `{` is typed in C-family
//!   languages: if the current line is pure whitespace (freshly
//!   auto-indented after a control keyword), return how many bytes to
//!   remove so the brace aligns with that keyword.

/// Compute the indentation string for the new line that follows `line`.
///
/// `at_eol` must be `true` when the cursor sits at or past the last
/// non-newline character of `line` (i.e. Enter at end-of-line or `o`).
/// When `false` (mid-line split or `O`) only basic autoindent applies.
pub fn next_line_indent(
    line: &str,
    at_eol: bool,
    filetype: &str,
    tab_width: usize,
    expandtab: bool,
) -> String {
    let base = leading_whitespace(line).to_string();
    if !at_eol {
        return base;
    }
    let trimmed = line.trim_end_matches('\n').trim_end();
    let unit = indent_unit(tab_width, expandtab);
    match filetype {
        "c" | "cpp" | "java" | "javascript" | "typescript" | "go" => {
            if trimmed.ends_with('{') || trimmed.ends_with('(') {
                format!("{base}{unit}")
            } else if is_c_control_line(trimmed) || ends_with_word(trimmed, "else") {
                format!("{base}{unit}")
            } else {
                base
            }
        }
        "rust" => {
            if trimmed.ends_with('{') || trimmed.ends_with('(') {
                format!("{base}{unit}")
            } else {
                base
            }
        }
        "python" | "ruby" => {
            if trimmed.ends_with(':') {
                format!("{base}{unit}")
            } else {
                base
            }
        }
        "lua" => {
            if ends_with_word(trimmed, "then")
                || ends_with_word(trimmed, "do")
                || ends_with_word(trimmed, "else")
                || ends_with_word(trimmed, "repeat")
                || (trimmed.contains("function") && trimmed.ends_with(')'))
            {
                format!("{base}{unit}")
            } else {
                base
            }
        }
        "sh" => {
            if ends_with_word(trimmed, "then") || ends_with_word(trimmed, "do") {
                format!("{base}{unit}")
            } else {
                base
            }
        }
        _ => base,
    }
}

/// For C-family languages: if `line_before_brace` is pure whitespace,
/// return the number of bytes to delete from the line start (one
/// shiftwidth) so `{` or `}` aligns with the surrounding block.
/// Returns 0 when no dedent should happen.
pub fn brace_dedent(line_before_brace: &str, filetype: &str, tab_width: usize) -> usize {
    if !matches!(
        filetype,
        "c" | "cpp" | "java" | "javascript" | "typescript" | "go" | "rust"
    ) {
        return 0;
    }
    if line_before_brace.is_empty()
        || !line_before_brace.chars().all(|c| c == ' ' || c == '\t')
    {
        return 0;
    }
    leading_indent_bytes_to_drop(line_before_brace, tab_width)
}

/// Smart backspace: when the cursor sits inside pure leading whitespace,
/// snap to the previous tab stop instead of deleting one character.
///
/// Returns `(bytes_to_delete, new_display_col)`. For non-whitespace
/// context returns `(1, col.saturating_sub(1))` — identical to a plain
/// backspace so the caller doesn't need to special-case.
pub fn smart_backspace(before_cursor: &str, current_col: usize, tab_width: usize) -> (usize, usize) {
    let tw = tab_width.max(1);
    if before_cursor.is_empty()
        || !before_cursor.chars().all(|c| c == ' ' || c == '\t')
    {
        return (1, current_col.saturating_sub(1));
    }
    // Snap to the previous tab stop: floor((col - 1) / tw) * tw
    let target_col = (current_col.saturating_sub(1) / tw) * tw;
    let target_byte = byte_at_display_col(before_cursor, target_col, tw);
    let bytes_to_remove = (before_cursor.len() - target_byte).max(1);
    (bytes_to_remove, target_col)
}

/// Return the byte offset in `s` at which display column `target` is reached.
fn byte_at_display_col(s: &str, target: usize, tab_width: usize) -> usize {
    let mut col = 0usize;
    let mut byte = 0usize;
    for b in s.bytes() {
        if col >= target {
            break;
        }
        match b {
            b' ' => {
                col += 1;
                byte += 1;
            }
            b'\t' => {
                col += tab_width - (col % tab_width);
                byte += 1;
            }
            _ => break,
        }
    }
    byte
}

// ---- helpers ----------------------------------------------------------------

/// Extract the leading spaces / tabs from `line`.
pub fn leading_whitespace(line: &str) -> &str {
    let n = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..n]
}

fn indent_unit(tab_width: usize, expandtab: bool) -> String {
    if expandtab {
        " ".repeat(tab_width.max(1))
    } else {
        "\t".to_string()
    }
}

/// True if `trimmed` is a C-family control-keyword line whose condition
/// ends with `)` with no terminating `{` or `;` — meaning the body
/// starts on the next line.
fn is_c_control_line(trimmed: &str) -> bool {
    if !trimmed.ends_with(')') {
        return false;
    }
    let c = trimmed.trim_start();
    c.starts_with("if ")
        || c.starts_with("if(")
        || c.starts_with("for ")
        || c.starts_with("for(")
        || c.starts_with("while ")
        || c.starts_with("while(")
        || c.starts_with("else if ")
        || c.starts_with("else if(")
        || c.starts_with("switch ")
        || c.starts_with("switch(")
}

/// True if `s` ends with `word` as a whole word (not suffix of a longer
/// identifier).
fn ends_with_word(s: &str, word: &str) -> bool {
    if !s.ends_with(word) {
        return false;
    }
    let rest = &s[..s.len() - word.len()];
    rest.is_empty() || !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_')
}

/// Number of bytes of leading whitespace representing one shiftwidth.
/// Mirrors `indent_actions::leading_indent_bytes_to_drop`.
fn leading_indent_bytes_to_drop(line: &str, tab_width: usize) -> usize {
    let tw = tab_width.max(1);
    let mut col = 0usize;
    let mut byte = 0usize;
    for b in line.bytes() {
        if col >= tw {
            break;
        }
        match b {
            b' ' => {
                col += 1;
                byte += 1;
            }
            b'\t' => {
                col += tw - (col % tw);
                byte += 1;
            }
            _ => break,
        }
    }
    byte
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_base_indent() {
        assert_eq!(next_line_indent("    foo;", true, "c", 4, true), "    ");
    }

    #[test]
    fn c_for_loop_adds_indent() {
        assert_eq!(
            next_line_indent("    for (i = 0; i < 10; i++)", true, "c", 4, true),
            "        "
        );
    }

    #[test]
    fn c_if_adds_indent() {
        assert_eq!(next_line_indent("    if (x > 0)", true, "c", 4, true), "        ");
    }

    #[test]
    fn c_open_brace_adds_indent() {
        assert_eq!(next_line_indent("    for (...) {", true, "c", 4, true), "        ");
    }

    #[test]
    fn c_else_adds_indent() {
        assert_eq!(next_line_indent("    } else", true, "c", 4, true), "        ");
    }

    #[test]
    fn c_semicolon_no_extra() {
        assert_eq!(next_line_indent("    int i;", true, "c", 4, true), "    ");
    }

    #[test]
    fn mid_line_no_smartindent() {
        assert_eq!(
            next_line_indent("    for (i = 0; i < 10; i++)", false, "c", 4, true),
            "    "
        );
    }

    #[test]
    fn python_colon_adds_indent() {
        assert_eq!(next_line_indent("    if True:", true, "python", 4, true), "        ");
    }

    #[test]
    fn python_no_colon_no_extra() {
        assert_eq!(next_line_indent("    x = 1", true, "python", 4, true), "    ");
    }

    #[test]
    fn lua_then_adds_indent() {
        assert_eq!(next_line_indent("    if x then", true, "lua", 4, true), "        ");
    }

    #[test]
    fn sh_do_adds_indent() {
        assert_eq!(next_line_indent("    for i in *; do", true, "sh", 4, true), "        ");
    }

    #[test]
    fn brace_dedent_spaces() {
        assert_eq!(brace_dedent("        ", "c", 4), 4);
    }

    #[test]
    fn brace_dedent_tab() {
        assert_eq!(brace_dedent("\t\t", "c", 4), 1);
    }

    #[test]
    fn brace_no_dedent_nonempty() {
        assert_eq!(brace_dedent("    x", "c", 4), 0);
    }

    #[test]
    fn brace_no_dedent_python() {
        assert_eq!(brace_dedent("        ", "python", 4), 0);
    }

    #[test]
    fn brace_no_dedent_empty() {
        assert_eq!(brace_dedent("", "c", 4), 0);
    }

    // ---- smart_backspace ----------------------------------------------------

    #[test]
    fn smart_bs_snaps_to_prev_tab_stop() {
        // 8 spaces, tw=4 → snap to 4
        let (n, col) = smart_backspace("        ", 8, 4);
        assert_eq!(n, 4);
        assert_eq!(col, 4);
    }

    #[test]
    fn smart_bs_from_tab_stop_snaps_to_zero() {
        // 4 spaces at col=4, tw=4 → snap to 0
        let (n, col) = smart_backspace("    ", 4, 4);
        assert_eq!(n, 4);
        assert_eq!(col, 0);
    }

    #[test]
    fn smart_bs_partial_indent() {
        // 3 spaces, tw=4 → snap to 0 (previous tab stop before col 3)
        let (n, col) = smart_backspace("   ", 3, 4);
        assert_eq!(n, 3);
        assert_eq!(col, 0);
    }

    #[test]
    fn smart_bs_non_whitespace_regular() {
        // "    foo" up to col 7: not pure whitespace → regular backspace
        let (n, col) = smart_backspace("    foo", 7, 4);
        assert_eq!(n, 1);
        assert_eq!(col, 6);
    }

    #[test]
    fn smart_bs_empty_is_regular() {
        let (n, col) = smart_backspace("", 0, 4);
        assert_eq!(n, 1);
        assert_eq!(col, 0);
    }
}
