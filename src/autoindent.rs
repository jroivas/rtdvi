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
///
/// `autoindent` — when `false`, returns an empty string (no indent at all).
/// `smartindent` — when `false`, returns only the base indent (no smart rules).
pub fn next_line_indent(
    line: &str,
    at_eol: bool,
    filetype: &str,
    tab_width: usize,
    expandtab: bool,
    autoindent: bool,
    smartindent: bool,
) -> String {
    if !autoindent {
        return String::new();
    }
    let base = leading_whitespace(line).to_string();
    // Comment-leader continuation (`/*` → ` * `, `//` → `// `, …) is a smart
    // rule, so it is gated on smartindent. Unlike the block-indent rules below
    // it also fires mid-line, so splitting a comment carries the leader onto
    // the new half rather than only when opening a line at EOL.
    if smartindent {
        if let Some(leader) = comment_continuation(line, filetype) {
            return leader;
        }
    }
    if !at_eol || !smartindent {
        return base;
    }
    let trimmed = line.trim_end_matches('\n').trim_end();
    let unit = indent_unit(tab_width, expandtab);
    // Continuation inside an unclosed `(` aligns to just after the paren — i.e.
    // under the first argument — like vim's default `cindent`. This takes
    // precedence over the block rules below (e.g. a line ending in `,`).
    if matches!(
        filetype,
        "c" | "cpp" | "java" | "javascript" | "typescript" | "go" | "rust"
    ) {
        if let Some(col) = open_paren_align_col(line, tab_width) {
            return " ".repeat(col);
        }
    }
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

/// Comment-leader continuation for a new line opened from `line` (Enter / `o`
/// / `O`). Returns the full leading string — indent plus comment marker — that
/// the new line should start with, or `None` when `line` is not inside a
/// continuable comment.
///
/// Handles C-family block comments (`/* … * … */`) and line comments (`//`,
/// plus Rust's `///` and `//!` doc markers). The leader is only recognised at
/// the *start* of the line, so trailing comments (`x = 1; // note`) do not
/// trigger continuation. Block continuation stops once the line also closes the
/// comment (`*/`), matching vim's `comments`/`formatoptions` behaviour without
/// any runtime files.
pub fn comment_continuation(line: &str, filetype: &str) -> Option<String> {
    if !matches!(
        filetype,
        "c" | "cpp" | "java" | "javascript" | "typescript" | "go" | "rust"
    ) {
        return None;
    }
    let base = leading_whitespace(line);
    let trimmed = line.trim();

    // Block comment. An opening `/* …` continues only if it does not also close
    // on the same line; a middle `* …` line continues unless it closes the
    // block. The continuation `*` is aligned one column in from the `/`, giving
    // the conventional `/*\n * \n */` layout regardless of indent.
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return if rest.contains("*/") {
            None // single-line `/* … */`, already closed
        } else {
            Some(format!("{base} * "))
        };
    }
    if trimmed.starts_with('*') {
        return if trimmed.contains("*/") {
            None // `*/` or `* … */` closes the block
        } else {
            Some(format!("{base}* "))
        };
    }

    // Line comments. Rust doc markers must be checked before the generic `//`.
    if filetype == "rust" {
        for marker in ["///", "//!"] {
            if trimmed.starts_with(marker) {
                return Some(format!("{base}{marker} "));
            }
        }
    }
    if trimmed.starts_with("//") {
        return Some(format!("{base}// "));
    }
    None
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

/// If `line` ends inside an unclosed `(` that has content after it, return the
/// display column the continuation line should indent to — one past that
/// paren, so the next argument lines up under the first. Returns `None` when
/// there is no open paren, or the innermost open paren is the last non-blank
/// character (a trailing `(`, which the block-indent rules handle instead).
///
/// Parens inside string/char literals and after a `//` line comment are
/// ignored. `[`/`{` are not tracked — only `(` alignment is intended — but a
/// `)` still pops, so a balanced nested call (`foo(bar(x),`) aligns to the
/// outer paren.
fn open_paren_align_col(line: &str, tab_width: usize) -> Option<usize> {
    let tw = tab_width.max(1);
    // Stack of unclosed `(`: (display column just after the paren, byte index
    // just after the paren).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut col = 0usize;
    let mut in_str = false;
    let mut in_char = false;
    let mut escaped = false;

    let mut it = line.char_indices().peekable();
    while let Some((bi, c)) = it.next() {
        // A `//` outside a literal starts a line comment — stop scanning.
        if !in_str && !in_char && c == '/' && matches!(it.peek(), Some((_, '/'))) {
            break;
        }
        let cw = if c == '\t' { tw - (col % tw) } else { 1 };
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if in_char {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '\'' => in_char = true,
                '(' => stack.push((col + cw, bi + c.len_utf8())),
                ')' => {
                    stack.pop();
                }
                _ => {}
            }
        }
        col += cw;
    }

    let (align_col, byte_after) = *stack.last()?;
    // Require real content after the paren (the arguments); a bare trailing
    // `(` should fall through to the normal block indent.
    let tail = &line[byte_after..];
    let tail = tail.split("//").next().unwrap_or(tail);
    if tail.trim().is_empty() {
        return None;
    }
    Some(align_col)
}

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

    fn ni(line: &str, at_eol: bool, ft: &str) -> String {
        next_line_indent(line, at_eol, ft, 4, true, true, true)
    }

    #[test]
    fn copies_base_indent() {
        assert_eq!(ni("    foo;", true, "c"), "    ");
    }

    #[test]
    fn block_comment_open_continues() {
        assert_eq!(comment_continuation("/**", "c").as_deref(), Some(" * "));
        assert_eq!(comment_continuation("/* hi", "c").as_deref(), Some(" * "));
    }

    #[test]
    fn block_comment_middle_continues_with_alignment() {
        assert_eq!(
            comment_continuation(" * Multiline comment", "c").as_deref(),
            Some(" * ")
        );
        assert_eq!(
            comment_continuation("    /**", "c").as_deref(),
            Some("     * ")
        );
        assert_eq!(
            comment_continuation("     * x", "c").as_deref(),
            Some("     * ")
        );
    }

    #[test]
    fn block_comment_closing_or_single_line_stops() {
        assert_eq!(comment_continuation(" */", "c"), None);
        assert_eq!(comment_continuation(" * done */", "c"), None);
        assert_eq!(comment_continuation("/* one line */", "c"), None);
    }

    #[test]
    fn line_comment_continues() {
        assert_eq!(comment_continuation("// note", "c").as_deref(), Some("// "));
        assert_eq!(comment_continuation("    // x", "rust").as_deref(), Some("    // "));
    }

    #[test]
    fn rust_doc_markers_continue() {
        assert_eq!(comment_continuation("/// docs", "rust").as_deref(), Some("/// "));
        assert_eq!(comment_continuation("//! inner", "rust").as_deref(), Some("//! "));
        // `///` is just a `//` comment in C, not a doc marker.
        assert_eq!(comment_continuation("/// x", "c").as_deref(), Some("// "));
    }

    #[test]
    fn trailing_comment_does_not_continue() {
        assert_eq!(comment_continuation("    x = 1; // note", "c"), None);
        assert_eq!(comment_continuation("foo(); /* tail", "c"), None);
    }

    #[test]
    fn comments_ignored_for_non_c_family() {
        assert_eq!(comment_continuation("// note", "python"), None);
        assert_eq!(comment_continuation("/* x", "lua"), None);
    }

    #[test]
    fn next_line_indent_carries_comment_leader() {
        // `o` on a middle comment line carries ` * ` rather than bare indent.
        assert_eq!(ni(" * Multiline comment", true, "c"), " * ");
        // mid-line split also continues (at_eol = false).
        assert_eq!(ni(" * Multiline comment", false, "c"), " * ");
    }

    #[test]
    fn c_for_loop_adds_indent() {
        assert_eq!(ni("    for (i = 0; i < 10; i++)", true, "c"), "        ");
    }

    #[test]
    fn open_paren_aligns_continuation_to_first_arg() {
        // `int something(` → `(` at column 13, so the next parameter lines up
        // at column 14.
        assert_eq!(ni("int something(int val1,", true, "c"), " ".repeat(14));
    }

    #[test]
    fn open_paren_alignment_respects_indent() {
        // Leading indent counts toward the paren column: `(` at column 7.
        assert_eq!(ni("    foo(bar,", true, "c"), " ".repeat(8));
    }

    #[test]
    fn trailing_open_paren_uses_block_indent_not_alignment() {
        // Nothing after `(` → keep the one-level block indent, not alignment.
        assert_eq!(ni("foo(", true, "c"), "    ");
    }

    #[test]
    fn balanced_parens_do_not_trigger_alignment() {
        // `if (x > 0)` is balanced, so the control-line rule adds one level.
        assert_eq!(ni("if (x > 0)", true, "c"), "    ");
    }

    #[test]
    fn nested_call_aligns_to_outer_open_paren() {
        // Inner `bar(x)` is balanced; align under the outer `(` at column 4.
        assert_eq!(ni("foo(bar(x), ", true, "c"), " ".repeat(4));
    }

    #[test]
    fn paren_inside_string_is_ignored() {
        // The `(` in the string literal must not count — printf's `(` (col 7)
        // is the open one.
        assert_eq!(ni(r#"printf("hi (","#, true, "c"), " ".repeat(7));
    }

    #[test]
    fn open_paren_alignment_works_for_rust() {
        assert_eq!(ni("fn foo(a: i32,", true, "rust"), " ".repeat(7));
    }

    #[test]
    fn open_paren_alignment_needs_smartindent() {
        // With smartindent off, only the base indent is copied (no alignment).
        assert_eq!(
            next_line_indent("    foo(bar,", true, "c", 4, true, true, false),
            "    "
        );
    }

    #[test]
    fn c_if_adds_indent() {
        assert_eq!(ni("    if (x > 0)", true, "c"), "        ");
    }

    #[test]
    fn c_open_brace_adds_indent() {
        assert_eq!(ni("    for (...) {", true, "c"), "        ");
    }

    #[test]
    fn c_else_adds_indent() {
        assert_eq!(ni("    } else", true, "c"), "        ");
    }

    #[test]
    fn c_semicolon_no_extra() {
        assert_eq!(ni("    int i;", true, "c"), "    ");
    }

    #[test]
    fn mid_line_no_smartindent() {
        assert_eq!(ni("    for (i = 0; i < 10; i++)", false, "c"), "    ");
    }

    #[test]
    fn python_colon_adds_indent() {
        assert_eq!(ni("    if True:", true, "python"), "        ");
    }

    #[test]
    fn python_no_colon_no_extra() {
        assert_eq!(ni("    x = 1", true, "python"), "    ");
    }

    #[test]
    fn lua_then_adds_indent() {
        assert_eq!(ni("    if x then", true, "lua"), "        ");
    }

    #[test]
    fn sh_do_adds_indent() {
        assert_eq!(ni("    for i in *; do", true, "sh"), "        ");
    }

    #[test]
    fn autoindent_off_returns_empty() {
        assert_eq!(
            next_line_indent("    for (i = 0; i < 10; i++)", true, "c", 4, true, false, true),
            ""
        );
    }

    #[test]
    fn smartindent_off_returns_base_only() {
        assert_eq!(
            next_line_indent("    for (i = 0; i < 10; i++)", true, "c", 4, true, true, false),
            "    "
        );
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
