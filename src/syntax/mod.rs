//! Syntax highlighting.
//!
//! Two layers feed the highlighter:
//!  1. A built-in layer with regex patterns for strings, numbers, comments,
//!     function calls, and **keyword lists** for the most common languages.
//!     Always applied — no external files required.
//!  2. The vim `syn keyword` definitions parsed out of
//!     `/usr/share/vim/vim*/syntax/<lang>.vim` (and similar lookup paths).
//!     Adds the long tail of language keywords not covered by layer 1.
//!     Optional — rtdvi works without a system vim installation.
//!
//! Vim's full syntax engine (regions, regex flavour, `contains=`, etc.) is
//! out of scope — only `syn keyword` and `hi link` are honoured. Keywords
//! cover most of the visual signal; the regex layer handles the rest.

use std::path::Path;

use regex::Regex;

mod detect;
mod rules;
mod state;

pub use detect::{detect_filetype, detect_filetype_for, normalize_filetype, FiletypeOverrides};
pub use rules::compile_vim_keywords;

use rules::{builtin_rules, find_vim_syntax_file};
use state::{
    advance_c_block_state, advance_python_state, find_c_block_open, find_python_triple_open,
    uses_c_block_comments,
};

/// Tracks whether successive lines are inside a multi-line construct.
/// Used for Python triple-quoted strings and C-style `/* … */` block
/// comments; languages with neither always stay at `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultilineState {
    #[default]
    None,
    TripleDouble, // inside """..."""
    TripleSingle, // inside '''...'''
    BlockComment, // inside /* ... */ (C, C++, Rust, Go, Java, JS/TS, CSS)
}

#[derive(Debug)]
pub struct Rule {
    pub regex: Regex,
    pub group: String,
    /// When `Some(i)`, paint only capture group `i` instead of the full
    /// match. Used for patterns that need surrounding context to anchor
    /// the match — e.g. `(\w+)\s*\(` finds function calls but we only want
    /// to colour the identifier, not the whitespace + paren.
    pub capture: Option<usize>,
}

/// Compiled rules for one filetype. Pattern order = priority (later wins).
pub struct Syntax {
    pub filetype: &'static str,
    pub rules: Vec<Rule>,
    /// `syn keyword` words flattened into a single regex per group, so we
    /// can match them as a single rule rather than thousands of literals.
    pub keyword_regexes: Vec<(Regex, String)>,
}

impl Syntax {
    /// Build a syntax engine for an unknown path with no overrides — the
    /// simplest entry point, suitable for tests.
    pub fn for_path(path: Option<&Path>) -> Self {
        Self::resolve("generic", path)
    }

    /// Build a syntax engine for a buffer, taking into account:
    /// - any manual `syntax_override` set via `:set syntax=…`,
    /// - the user's `[filetypes]` glob overrides from config,
    /// - the built-in extension table,
    /// - and `mime_guess` as last resort.
    pub fn for_buffer(
        path: Option<&Path>,
        manual_override: Option<&str>,
        overrides: &FiletypeOverrides,
    ) -> Self {
        let filetype = if let Some(name) = manual_override {
            normalize_filetype(name)
        } else if let Some(p) = path {
            detect_filetype_for(p, overrides)
        } else {
            "generic".into()
        };
        Self::resolve_dyn(&filetype, path)
    }

    fn resolve(filetype: &'static str, path: Option<&Path>) -> Self {
        let mut syn = Self {
            filetype,
            rules: builtin_rules(filetype),
            keyword_regexes: Vec::new(),
        };
        if let Some(p) = find_vim_syntax_file(filetype) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                syn.keyword_regexes = compile_vim_keywords(&text);
            }
        }
        let _ = path;
        syn
    }

    fn resolve_dyn(filetype: &str, path: Option<&Path>) -> Self {
        // Explicitly disabled (`:set syntax=off` / `none`): no rules, no
        // keywords — the buffer renders with zero syntax highlighting.
        if matches!(filetype, "off" | "none" | "disabled") {
            return Self {
                filetype: "off",
                rules: Vec::new(),
                keyword_regexes: Vec::new(),
            };
        }
        // `filetype` here is `String`-owned; we need a `'static` slot on the
        // struct for compatibility. Stash a known-static slug ("custom") and
        // rely on the loaded rules + keyword regexes for actual highlighting.
        let static_ft = match filetype {
            "rust" => "rust",
            "python" => "python",
            "markdown" => "markdown",
            "toml" => "toml",
            "yaml" => "yaml",
            "json" => "json",
            "c" => "c",
            "cpp" => "cpp",
            "javascript" => "javascript",
            "typescript" => "typescript",
            "sh" => "sh",
            "vim" => "vim",
            "html" => "html",
            "css" => "css",
            "go" => "go",
            "lua" => "lua",
            "ruby" => "ruby",
            "java" => "java",
            "tex" => "tex",
            "make" => "make",
            "dockerfile" => "dockerfile",
            "gitconfig" => "gitconfig",
            _ => "generic",
        };
        let mut syn = Self {
            filetype: static_ft,
            rules: builtin_rules(static_ft),
            keyword_regexes: Vec::new(),
        };
        // Always try to locate a vim syntax file by the resolved name — the
        // system install ships hundreds, so unrecognised names that map
        // straight to a `.vim` file (e.g. ones rtdvi's builtin map misses)
        // still get keyword highlighting for free.
        if let Some(p) = find_vim_syntax_file(filetype) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                syn.keyword_regexes = compile_vim_keywords(&text);
            }
        }
        let _ = path;
        syn
    }

    /// Compute non-overlapping `(byte_range, group)` spans for one line.
    /// Later rules override earlier ones (so e.g. `Comment` covering `// foo`
    /// wins over a `Keyword` rule that would otherwise highlight `foo`).
    pub fn highlight_line(&self, line: &str) -> Vec<(std::ops::Range<usize>, String)> {
        if line.is_empty() {
            return Vec::new();
        }
        // Per-byte assignment of (priority, group). Bigger priority wins.
        let mut per_byte: Vec<Option<(u32, &str)>> = vec![None; line.len()];
        // Keyword regexes are lowest priority; everything else (strings,
        // comments, numbers in `rules`) overrides them.
        for (regex, group) in &self.keyword_regexes {
            for m in regex.find_iter(line) {
                paint(&mut per_byte, m.start(), m.end(), 0, group);
            }
        }
        for (prio, rule) in self.rules.iter().enumerate() {
            let priority = (prio + 1) as u32;
            match rule.capture {
                None => {
                    for m in rule.regex.find_iter(line) {
                        paint(&mut per_byte, m.start(), m.end(), priority, &rule.group);
                    }
                }
                Some(idx) => {
                    for caps in rule.regex.captures_iter(line) {
                        if let Some(m) = caps.get(idx) {
                            paint(&mut per_byte, m.start(), m.end(), priority, &rule.group);
                        }
                    }
                }
            }
        }
        // Coalesce consecutive same-group cells into ranges.
        let mut out = Vec::new();
        let mut i = 0;
        while i < per_byte.len() {
            let g = per_byte[i].map(|(_, g)| g);
            let start = i;
            while i < per_byte.len() && per_byte[i].map(|(_, g)| g) == g {
                i += 1;
            }
            if let Some(group) = g {
                out.push((start..i, group.to_string()));
            }
        }
        out
    }

    /// Advance `state` through `line`, returning the state at end-of-line.
    /// Tracks Python triple-quoted strings and C-style block comments;
    /// other languages always return `None`.
    pub fn advance_state(&self, line: &str, state: MultilineState) -> MultilineState {
        if self.filetype == "python" {
            advance_python_state(line, state)
        } else if uses_c_block_comments(self.filetype) {
            advance_c_block_state(line, state)
        } else {
            MultilineState::None
        }
    }

    /// Like [`highlight_line`] but aware of multi-line string / comment
    /// context. Returns `(spans, state_after_line)`.
    pub fn highlight_line_ctx(
        &self,
        line: &str,
        state: MultilineState,
    ) -> (Vec<(std::ops::Range<usize>, String)>, MultilineState) {
        if uses_c_block_comments(self.filetype) {
            return self.highlight_block_comment_ctx(line, state);
        }
        if self.filetype != "python" {
            return (self.highlight_line(line), MultilineState::None);
        }
        match state {
            MultilineState::TripleDouble | MultilineState::TripleSingle => {
                let delim: &[u8] = if state == MultilineState::TripleDouble {
                    b"\"\"\""
                } else {
                    b"'''"
                };
                let bytes = line.as_bytes();
                let close_pos = (0..bytes.len().saturating_sub(2))
                    .find(|&k| bytes.get(k..k + 3) == Some(delim));
                if let Some(pos) = close_pos {
                    let end = pos + 3;
                    let mut spans = vec![(0..end, "String".to_string())];
                    let rest = &line[end..];
                    let (rest_spans, next) = self.highlight_line_ctx(rest, MultilineState::None);
                    for (r, g) in rest_spans {
                        spans.push((r.start + end..r.end + end, g));
                    }
                    (spans, next)
                } else {
                    let spans = if !line.is_empty() {
                        vec![(0..line.len(), "String".to_string())]
                    } else {
                        vec![]
                    };
                    (spans, state)
                }
            }
            MultilineState::None | MultilineState::BlockComment => {
                let base = self.highlight_line(line);
                let next = advance_python_state(line, MultilineState::None);
                if next == MultilineState::None {
                    return (base, MultilineState::None);
                }
                // An unclosed triple-string opens on this line — find where.
                let delim: &[u8] = if next == MultilineState::TripleDouble {
                    b"\"\"\""
                } else {
                    b"'''"
                };
                let open_pos = find_python_triple_open(line, delim).unwrap_or(0);
                // Keep base spans that fall entirely before the opening delimiter.
                let mut spans: Vec<(std::ops::Range<usize>, String)> = base
                    .into_iter()
                    .filter_map(|(r, g)| {
                        if r.end <= open_pos {
                            Some((r, g))
                        } else if r.start < open_pos {
                            Some((r.start..open_pos, g))
                        } else {
                            None
                        }
                    })
                    .collect();
                spans.push((open_pos..line.len(), "String".to_string()));
                (spans, next)
            }
        }
    }

    /// Highlight one line of a C-style language, threading `/* … */` block
    /// comment state across lines. Returns `(spans, state_after_line)`.
    fn highlight_block_comment_ctx(
        &self,
        line: &str,
        state: MultilineState,
    ) -> (Vec<(std::ops::Range<usize>, String)>, MultilineState) {
        if state == MultilineState::BlockComment {
            // Already inside a comment: paint up to the closing `*/`, then
            // resume normal highlighting on whatever follows it.
            let bytes = line.as_bytes();
            let close = (0..bytes.len().saturating_sub(1))
                .find(|&k| bytes.get(k..k + 2) == Some(b"*/"));
            if let Some(pos) = close {
                let end = pos + 2;
                let mut spans = vec![(0..end, "Comment".to_string())];
                let rest = &line[end..];
                let (rest_spans, next) =
                    self.highlight_block_comment_ctx(rest, MultilineState::None);
                for (r, g) in rest_spans {
                    spans.push((r.start + end..r.end + end, g));
                }
                return (spans, next);
            }
            let spans = if line.is_empty() {
                vec![]
            } else {
                vec![(0..line.len(), "Comment".to_string())]
            };
            return (spans, MultilineState::BlockComment);
        }

        // Not inside a comment: highlight normally, then check whether an
        // unterminated `/*` opens on this line.
        let base = self.highlight_line(line);
        if advance_c_block_state(line, MultilineState::None) != MultilineState::BlockComment {
            return (base, MultilineState::None);
        }
        let open_pos = find_c_block_open(line).unwrap_or(0);
        // Keep base spans before the opening delimiter (truncating any that
        // straddle it); everything from `/*` to end-of-line is the comment.
        let mut spans: Vec<(std::ops::Range<usize>, String)> = base
            .into_iter()
            .filter_map(|(r, g)| {
                if r.end <= open_pos {
                    Some((r, g))
                } else if r.start < open_pos {
                    Some((r.start..open_pos, g))
                } else {
                    None
                }
            })
            .collect();
        spans.push((open_pos..line.len(), "Comment".to_string()));
        (spans, MultilineState::BlockComment)
    }
}

fn paint<'a>(buf: &mut [Option<(u32, &'a str)>], start: usize, end: usize, prio: u32, group: &'a str) {
    for i in start..end.min(buf.len()) {
        if buf[i].map_or(true, |(p, _)| prio >= p) {
            buf[i] = Some((prio, group));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_extensions() {
        assert_eq!(detect_filetype(Path::new("foo.rs")), "rust");
        assert_eq!(detect_filetype(Path::new("README.md")), "markdown");
        assert_eq!(detect_filetype(Path::new("config.toml")), "toml");
        assert_eq!(detect_filetype(Path::new("Makefile")), "make");
        assert_eq!(detect_filetype(Path::new("notes.txt")), "generic");
    }

    #[test]
    fn generic_finds_double_quoted_strings_and_numbers() {
        let syn = Syntax {
            filetype: "rust",
            rules: builtin_rules("rust"),
            keyword_regexes: Vec::new(),
        };
        let tokens = syn.highlight_line(r#"let x = 42; let s = "hi"; // tail"#);
        let groups: Vec<&str> = tokens.iter().map(|(_, g)| g.as_str()).collect();
        assert!(groups.contains(&"Number"));
        assert!(groups.contains(&"String"));
        assert!(groups.contains(&"Comment"));
    }

    #[test]
    fn comment_wins_over_keyword_inside_line() {
        let kws = compile_vim_keywords("syn keyword fooKeyword fn let\nhi link fooKeyword Keyword");
        let syn = Syntax {
            filetype: "rust",
            rules: builtin_rules("rust"),
            keyword_regexes: kws,
        };
        let tokens = syn.highlight_line(r#"// fn let"#);
        // Whole line should be Comment, not Keyword.
        for (_, g) in &tokens {
            assert_eq!(g, "Comment");
        }
    }

    #[test]
    fn c_preprocessor_and_function_calls_are_highlighted() {
        let syn = Syntax {
            filetype: "c",
            rules: builtin_rules("c"),
            keyword_regexes: Vec::new(),
        };
        let line = r#"#include <stdio.h>"#;
        let tokens = syn.highlight_line(line);
        let groups: Vec<&str> = tokens.iter().map(|(_, g)| g.as_str()).collect();
        assert!(groups.contains(&"PreProc"), "groups = {groups:?}");

        let line = r#"    printf("hi %d", 5);"#;
        let tokens = syn.highlight_line(line);
        // The identifier "printf" should be Function; the "5" should be Number;
        // the `"hi %d"` should be String.
        let found_func = tokens.iter().any(|(r, g)| g == "Function" && &line[r.clone()] == "printf");
        let found_num = tokens.iter().any(|(_, g)| g == "Number");
        let found_str = tokens.iter().any(|(_, g)| g == "String");
        assert!(found_func, "no Function token for printf: {tokens:?}");
        assert!(found_num);
        assert!(found_str);
    }

    #[test]
    fn parse_vim_keywords_handles_options() {
        let kws =
            compile_vim_keywords("syn keyword rustKeyword pub use mod nextgroup=foo skipwhite\nhi def link rustKeyword Keyword");
        assert_eq!(kws.len(), 1);
        let (_, g) = &kws[0];
        assert_eq!(g, "Keyword");
    }

    #[test]
    fn python_triple_string_multiline_state() {
        let syn = Syntax {
            filetype: "python",
            rules: builtin_rules("python"),
            keyword_regexes: Vec::new(),
        };

        // Opening line: """ — starts a triple-double block
        let (spans, next) = syn.highlight_line_ctx("\"\"\"", MultilineState::None);
        assert_eq!(next, MultilineState::TripleDouble);
        assert!(spans.iter().any(|(_, g)| g == "String"), "opening line should be String: {spans:?}");

        // Interior line: completely inside the block
        let (spans, next) = syn.highlight_line_ctx("This is a docstring", MultilineState::TripleDouble);
        assert_eq!(next, MultilineState::TripleDouble, "interior stays in block");
        assert!(spans.iter().all(|(_, g)| g == "String"), "interior should be all String: {spans:?}");

        // Closing line: """ — ends the block
        let (spans, next) = syn.highlight_line_ctx("\"\"\"", MultilineState::TripleDouble);
        assert_eq!(next, MultilineState::None, "block should close");
        assert!(spans.iter().any(|(_, g)| g == "String"), "closing line should be String: {spans:?}");
    }

    #[test]
    fn c_block_comment_multiline_state() {
        let syn = Syntax {
            filetype: "c",
            rules: builtin_rules("c"),
            keyword_regexes: Vec::new(),
        };

        // Opening line: `/*` with no close — opens a block comment.
        let (spans, next) = syn.highlight_line_ctx("/*", MultilineState::None);
        assert_eq!(next, MultilineState::BlockComment);
        assert!(spans.iter().any(|(_, g)| g == "Comment"), "opening should be Comment: {spans:?}");

        // Interior line: entirely inside the comment, including code-looking text.
        let (spans, next) =
            syn.highlight_line_ctx(" * int main() { return 0; }", MultilineState::BlockComment);
        assert_eq!(next, MultilineState::BlockComment, "interior stays in block");
        assert!(
            spans.iter().all(|(_, g)| g == "Comment"),
            "interior should be all Comment (no keyword/number bleed): {spans:?}"
        );

        // Closing line: `*/` ends the block.
        let (spans, next) = syn.highlight_line_ctx(" */", MultilineState::BlockComment);
        assert_eq!(next, MultilineState::None, "block should close");
        assert!(spans.iter().any(|(_, g)| g == "Comment"), "closing should be Comment: {spans:?}");
    }

    #[test]
    fn c_block_comment_closes_then_code_on_same_line() {
        let syn = Syntax {
            filetype: "c",
            rules: builtin_rules("c"),
            keyword_regexes: Vec::new(),
        };
        // `*/ int x;` — comment closes, then real code follows and is no
        // longer comment-coloured.
        let (spans, next) = syn.highlight_line_ctx("*/ int x;", MultilineState::BlockComment);
        assert_eq!(next, MultilineState::None);
        // The `int` keyword after the close must NOT be Comment.
        let line = "*/ int x;";
        let int_is_keyword = spans
            .iter()
            .any(|(r, g)| g == "Keyword" && &line[r.start..r.end] == "int");
        assert!(int_is_keyword, "code after */ should be highlighted normally: {spans:?}");
    }

    #[test]
    fn c_block_comment_single_line_stays_none() {
        let syn = Syntax {
            filetype: "c",
            rules: builtin_rules("c"),
            keyword_regexes: Vec::new(),
        };
        // Opens and closes on one line → no lingering state.
        let (_, next) = syn.highlight_line_ctx("a /* x */ b", MultilineState::None);
        assert_eq!(next, MultilineState::None);
    }

    #[test]
    fn c_block_open_inside_string_is_ignored() {
        let syn = Syntax {
            filetype: "rust",
            rules: builtin_rules("rust"),
            keyword_regexes: Vec::new(),
        };
        // The `/*` lives inside a string literal — it must not open a comment.
        let (_, next) = syn.highlight_line_ctx(r#"let s = "/* not a comment";"#, MultilineState::None);
        assert_eq!(next, MultilineState::None, "/* inside a string should not open a block comment");
    }

    #[test]
    fn python_triple_string_single_line() {
        let syn = Syntax {
            filetype: "python",
            rules: builtin_rules("python"),
            keyword_regexes: Vec::new(),
        };
        // A triple-string that opens and closes on the same line
        let (_, next) = syn.highlight_line_ctx(r#""""docstring""""#, MultilineState::None);
        assert_eq!(next, MultilineState::None, "single-line triple-string should leave state None");
    }

    #[test]
    fn syntax_off_override_disables_highlighting() {
        let ov = FiletypeOverrides::default();
        let line = r#"let x = 42; // c"#;

        // `off` / `none` → no highlighting at all, even on an obvious .rs file.
        for disable in ["off", "none"] {
            let syn = Syntax::for_buffer(Some(Path::new("foo.rs")), Some(disable), &ov);
            assert_eq!(syn.filetype, "off");
            assert!(
                syn.highlight_line(line).is_empty(),
                "syntax={disable} should produce no spans"
            );
        }

        // No override (the `auto`/`on` case) → auto-detected rust, highlighted.
        let syn = Syntax::for_buffer(Some(Path::new("foo.rs")), None, &ov);
        assert_eq!(syn.filetype, "rust");
        assert!(!syn.highlight_line(line).is_empty(), "auto should highlight");
    }
}
