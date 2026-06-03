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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;

/// User-supplied `glob → filetype/MIME` rules from the TOML config.
/// Tried before built-in extension detection.
#[derive(Default, Debug, Clone)]
pub struct FiletypeOverrides {
    /// Each entry is `(compiled glob regex, raw rhs)`. The rhs may be a
    /// short filetype name (`rust`, `c++`) or a MIME type (`text/markdown`).
    rules: Vec<(Regex, String)>,
}

impl FiletypeOverrides {
    pub fn from_map(map: &HashMap<String, String>) -> Self {
        let mut rules = Vec::with_capacity(map.len());
        // Stable order: longest pattern first, then alphabetical. Longer
        // patterns are more specific (`Cargo.toml` beats `*.toml`).
        let mut sorted: Vec<(&String, &String)> = map.iter().collect();
        sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));
        for (glob, ft) in sorted {
            if let Ok(re) = glob_to_regex(glob) {
                rules.push((re, ft.clone()));
            }
        }
        Self { rules }
    }

    /// Return the matching rhs (un-normalised) for `path`'s basename, if any.
    pub fn match_path(&self, path: &Path) -> Option<&str> {
        let name = path.file_name()?.to_str()?;
        for (re, rhs) in &self.rules {
            if re.is_match(name) {
                return Some(rhs);
            }
        }
        None
    }
}

fn glob_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let mut out = String::from(r"\A");
    for c in pattern.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out.push_str(r"\z");
    Regex::new(&out)
}

/// Canonicalise a filetype string into the form rtdvi/vim uses internally.
/// Accepts vim-style names (`c++`, `cpp`), short aliases, AND MIME types
/// (`text/markdown`). Unknown MIME types fall through unchanged.
pub fn normalize_filetype(ft: &str) -> String {
    let trimmed = ft.trim();
    if let Some(mapped) = mime_to_filetype(trimmed) {
        return mapped.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "c++" | "cxx" | "c++src" | "c++hdr" | "cppsrc" | "cpphdr" => "cpp".into(),
        "objective-c" => "objc".into(),
        "js" | "node" => "javascript".into(),
        "ts" => "typescript".into(),
        "shellscript" | "bash" | "zsh" => "sh".into(),
        "py" => "python".into(),
        "markdown.pandoc" => "markdown".into(),
        _ => lower,
    }
}

/// Translate a MIME type (`text/x-rust`) into a filetype rtdvi understands.
/// `None` means we don't have a translation and the caller should keep the
/// MIME string as-is (which won't match any syntax file but is harmless).
fn mime_to_filetype(mime: &str) -> Option<&'static str> {
    if !mime.contains('/') {
        return None;
    }
    Some(match mime {
        "text/x-rust" | "application/x-rust" => "rust",
        "text/x-python" | "application/x-python" => "python",
        "text/markdown" | "text/x-markdown" => "markdown",
        "text/x-c" | "text/x-csrc" | "text/x-chdr" => "c",
        "text/x-c++" | "text/x-c++src" | "text/x-c++hdr" | "text/x-cpp" => "cpp",
        "text/x-java" | "text/x-java-source" => "java",
        "text/javascript" | "application/javascript" | "text/x-javascript" => "javascript",
        "application/typescript" | "text/x-typescript" => "typescript",
        "text/x-makefile" => "make",
        "text/x-shellscript" | "application/x-sh" | "application/x-shellscript" => "sh",
        "text/x-ruby" | "application/x-ruby" => "ruby",
        "text/x-lua" | "application/x-lua" => "lua",
        "text/x-go" | "application/x-go" => "go",
        "text/x-vim" => "vim",
        "application/json" | "text/json" => "json",
        "application/toml" | "text/x-toml" | "text/toml" => "toml",
        "text/yaml" | "application/x-yaml" | "text/x-yaml" => "yaml",
        "text/html" | "application/xhtml+xml" => "html",
        "text/css" => "css",
        "application/x-tex" | "text/x-tex" | "application/x-latex" => "tex",
        _ => return None,
    })
}

/// Detect a filetype for `path` honouring (in order):
///   1. User overrides from `[filetypes]` in config.
///   2. Built-in basename / extension table.
///   3. `mime_guess` extension → MIME → filetype translation.
/// Falls back to `"generic"`.
pub fn detect_filetype_for(path: &Path, overrides: &FiletypeOverrides) -> String {
    if let Some(rhs) = overrides.match_path(path) {
        return normalize_filetype(rhs);
    }
    if let Some(builtin) = builtin_detect(path) {
        return builtin.to_string();
    }
    // Last resort: ask `mime_guess` from the extension and translate back.
    let guesses = mime_guess::from_path(path);
    for mime in guesses.iter() {
        if let Some(mapped) = mime_to_filetype(mime.as_ref()) {
            return mapped.into();
        }
    }
    "generic".into()
}

/// No-overrides shortcut for callers (mostly tests). Returns the built-in
/// detection or `"generic"`.
pub fn detect_filetype(path: &Path) -> &'static str {
    builtin_detect(path).unwrap_or("generic")
}

fn builtin_detect(path: &Path) -> Option<&'static str> {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        match name {
            "Makefile" | "makefile" | "GNUmakefile" => return Some("make"),
            "Dockerfile" => return Some("dockerfile"),
            "Cargo.toml" | "Cargo.lock" => return Some("toml"),
            ".gitignore" | ".gitconfig" => return Some("gitconfig"),
            _ => {}
        }
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    Some(match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "sh" | "bash" => "sh",
        "vim" => "vim",
        "html" | "htm" => "html",
        "css" => "css",
        "go" => "go",
        "lua" => "lua",
        "rb" => "ruby",
        "java" => "java",
        "tex" => "tex",
        _ => return None,
    })
}

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

// ---- Multi-line block comment tracking (C-style /* ... */) ----------------

/// Languages whose `/* … */` block comments can span multiple lines and so
/// need cross-line state threading (the single-line `/\*.*?\*/` rule only
/// catches comments that open and close on the same line).
fn uses_c_block_comments(ft: &str) -> bool {
    matches!(
        ft,
        "c" | "cpp" | "rust" | "go" | "java" | "javascript" | "typescript" | "css"
    )
}

/// Advance block-comment `state` through one line, returning the state at
/// end-of-line. Skips `//` line comments and double-quoted strings so a
/// `/*` inside either doesn't spuriously open a comment.
fn advance_c_block_state(line: &str, mut state: MultilineState) -> MultilineState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == MultilineState::BlockComment {
            if bytes.get(i..i + 2) == Some(b"*/") {
                state = MultilineState::None;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        // Outside a comment.
        if bytes.get(i..i + 2) == Some(b"//") {
            break; // rest of line is a line comment — nothing left to open
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            state = MultilineState::BlockComment;
            i += 2;
        } else if bytes[i] == b'"' {
            i = skip_dq_string(bytes, i);
        } else {
            i += 1;
        }
    }
    if state == MultilineState::BlockComment {
        MultilineState::BlockComment
    } else {
        MultilineState::None
    }
}

/// Byte offset of the `/*` that opens an *unterminated* block comment on
/// `line` (one with no matching `*/` before end-of-line), or `None`.
fn find_c_block_open(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i..i + 2) == Some(b"//") {
            return None;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            let open = i;
            let mut j = i + 2;
            let mut closed = false;
            while j < bytes.len() {
                if bytes.get(j..j + 2) == Some(b"*/") {
                    closed = true;
                    j += 2;
                    break;
                }
                j += 1;
            }
            if !closed {
                return Some(open);
            }
            i = j;
        } else if bytes[i] == b'"' {
            i = skip_dq_string(bytes, i);
        } else {
            i += 1;
        }
    }
    None
}

/// Given `bytes[i] == b'"'`, return the index just past the closing quote
/// (honouring backslash escapes), or `bytes.len()` if unterminated.
fn skip_dq_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1; // skip opening quote
    while i < bytes.len() && bytes[i] != b'"' {
        if bytes[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    if i < bytes.len() {
        i += 1; // consume closing quote
    }
    i
}

// ---- Multi-line string tracking (Python triple-quoted strings) ------------

/// Advance `state` through one line, returning the state at the end of it.
fn advance_python_state(line: &str, mut state: MultilineState) -> MultilineState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match state {
            MultilineState::None => {
                if bytes[i] == b'#' {
                    break;
                }
                if bytes.get(i..i + 3) == Some(b"\"\"\"") {
                    state = MultilineState::TripleDouble;
                    i += 3;
                } else if bytes.get(i..i + 3) == Some(b"'''") {
                    state = MultilineState::TripleSingle;
                    i += 3;
                } else if bytes[i] == b'"' {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            MultilineState::TripleDouble => {
                if bytes.get(i..i + 3) == Some(b"\"\"\"") {
                    state = MultilineState::None;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            MultilineState::TripleSingle => {
                if bytes.get(i..i + 3) == Some(b"'''") {
                    state = MultilineState::None;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            // Python never enters block-comment state; treat defensively.
            MultilineState::BlockComment => i += 1,
        }
    }
    state
}

/// Find the byte position of the opening `delim` (`b"\"\"\""` or `b"'''"`)
/// in `line`, skipping over `#` comments. Returns `None` if not found.
fn find_python_triple_open(line: &str, delim: &[u8]) -> Option<usize> {
    let bytes = line.as_bytes();
    let limit = bytes.len().saturating_sub(2);
    for i in 0..limit {
        if bytes[i] == b'#' {
            return None;
        }
        if bytes.get(i..i + 3) == Some(delim) {
            return Some(i);
        }
    }
    None
}

// ---- Built-in regex rules per filetype ------------------------------------

fn builtin_rules(filetype: &str) -> Vec<Rule> {
    // (pattern, group, capture_index). capture=None paints the whole match.
    let mut specs: Vec<(&str, &str, Option<usize>)> = Vec::new();
    let dq_string = r#""(?:\\.|[^"\\])*""#;
    let sq_string = r#"'(?:\\.|[^'\\])*'"#;
    let number = r"\b\d+(?:\.\d+)?\b";
    // `(ident)` followed by `(` — function call. We capture the identifier
    // so trailing whitespace/`(` don't get the Function colour.
    let func_call = r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(";
    // `^\s*#\s*<word>` — C-family preprocessor line (include, define, …).
    // Greedy to end of line so the included path tags as PreProc too.
    let preproc = r"^\s*#\s*\w+.*$";

    // Keywords are added first so that string/comment rules added after them
    // take precedence (later rules win when spans overlap).
    match filetype {
        "c" => {
            specs.push((
                r"\b(auto|break|case|char|const|continue|default|do|double|else|enum|extern|float|for|goto|if|inline|int|long|register|return|short|signed|sizeof|static|struct|switch|typedef|union|unsigned|void|volatile|while)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "Character", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((preproc, "PreProc", None));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "cpp" => {
            specs.push((
                r"\b(alignas|alignof|and|and_eq|asm|auto|bitand|bitor|bool|break|case|catch|char|char8_t|char16_t|char32_t|class|compl|concept|const|consteval|constexpr|constinit|const_cast|continue|co_await|co_return|co_yield|decltype|default|delete|do|double|dynamic_cast|else|enum|explicit|export|extern|false|float|for|friend|goto|if|inline|int|long|mutable|namespace|new|noexcept|not|not_eq|nullptr|operator|or|or_eq|override|private|protected|public|register|reinterpret_cast|requires|return|short|signed|sizeof|static|static_assert|static_cast|struct|switch|template|this|thread_local|throw|true|try|typedef|typeid|typename|union|unsigned|using|virtual|void|volatile|wchar_t|while|xor|xor_eq)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "Character", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((preproc, "PreProc", None));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "rust" => {
            specs.push((
                r"\b(as|async|await|break|const|continue|crate|dyn|else|enum|extern|false|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|true|type|union|unsafe|use|where|while|abstract|become|box|do|final|macro|override|priv|try|typeof|unsized|virtual|yield)\b",
                "Keyword", None,
            ));
            // Primitive types
            specs.push((
                r"\b(bool|char|f32|f64|i8|i16|i32|i64|i128|isize|str|u8|u16|u32|u64|u128|usize)\b",
                "Type", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "go" => {
            specs.push((
                r"\b(break|case|chan|const|continue|default|defer|else|fallthrough|for|func|go|goto|if|import|interface|map|package|range|return|select|struct|switch|type|var)\b",
                "Keyword", None,
            ));
            specs.push((
                r"\b(bool|byte|complex64|complex128|error|float32|float64|int|int8|int16|int32|int64|rune|string|uint|uint8|uint16|uint32|uint64|uintptr)\b",
                "Type", None,
            ));
            specs.push((
                r"\b(append|cap|close|copy|delete|len|make|new|panic|print|println|real|recover|imag|true|false|nil|iota)\b",
                "Identifier", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "python" => {
            specs.push((
                r"\b(and|as|assert|async|await|break|class|continue|def|del|elif|else|except|False|finally|for|from|global|if|import|in|is|lambda|None|nonlocal|not|or|pass|raise|return|True|try|while|with|yield)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"#.*$", "Comment", None));
        }
        "javascript" => {
            specs.push((
                r"\b(async|await|break|case|catch|class|const|continue|debugger|default|delete|do|else|export|extends|false|finally|for|from|function|if|import|in|instanceof|let|new|null|of|return|static|super|switch|this|throw|true|try|typeof|undefined|var|void|while|with|yield|get|set)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "typescript" => {
            specs.push((
                r"\b(abstract|any|as|async|await|boolean|break|case|catch|class|const|constructor|continue|declare|default|delete|do|else|enum|export|extends|false|finally|for|from|function|if|implements|import|in|instanceof|interface|keyof|let|module|namespace|never|new|null|number|object|of|override|private|protected|public|readonly|return|static|string|super|switch|symbol|this|throw|true|try|type|typeof|undefined|unknown|var|void|while|with|yield|get|set)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "java" => {
            specs.push((
                r"\b(abstract|assert|boolean|break|byte|case|catch|char|class|const|continue|default|do|double|else|enum|extends|false|final|finally|float|for|goto|if|implements|import|instanceof|int|interface|long|native|new|null|package|private|protected|public|return|short|static|strictfp|super|switch|synchronized|this|throw|throws|transient|true|try|var|void|volatile|while|yield|record|sealed|permits|non-sealed)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "Character", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "sh" => {
            specs.push((
                r"\b(break|case|continue|do|done|elif|else|esac|exit|export|fi|for|function|if|in|local|readonly|return|select|shift|source|then|until|while)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"#.*$", "Comment", None));
        }
        "lua" => {
            specs.push((
                r"\b(and|break|do|else|elseif|end|false|for|function|goto|if|in|local|nil|not|or|repeat|return|then|true|until|while)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"--.*$", "Comment", None));
        }
        "ruby" => {
            specs.push((
                r"\b(alias|and|begin|break|case|class|def|defined\?|do|else|elsif|end|ensure|false|for|if|in|module|next|nil|not|or|redo|rescue|retry|return|self|super|then|true|undef|unless|until|when|while|yield)\b",
                "Keyword", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((func_call, "Function", Some(1)));
            specs.push((r"#.*$", "Comment", None));
        }
        "css" => {
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r"/\*.*?\*/", "Comment", None));
        }
        "vim" => {
            specs.push((
                r"\b(ab|abbreviate|abc|abclear|abo|aboveleft|al|all|ar|arga|argadd|argd|argdelete|arge|argedit|argg|argglobal|argl|arglocal|args|argu|argument|as|ascii|b|ba|bad|badd|ball|bd|bdelete|be|bel|belowright|bf|bfirst|bl|blast|bm|bmodified|bn|bnext|bo|botright|bp|bprevious|br|brea|break|breaka|breakadd|breakd|breakdel|breakl|breaklist|brewind|bro|browse|bufdo|buffer|buffers|bun|bunload|bw|bwipeout|c|cabc|cabclear|cad|caddb|caddbuffer|caddexpr|caddf|caddfile|cal|call|cat|catch|cb|cbuffer|cc|ccl|cclose|cd|ce|center|cex|cexpr|cf|cfile|cfir|cfirst|cg|cgetb|cgetbuffer|cgete|cgetexpr|cgetf|cgetfile|cgf|cgrepadd|cl|cla|clast|cle|clearjumps|clist|clo|close|cm|cmap|cmapc|cmapclear|cmenu|cn|cnew|cnewer|cNext|cnf|cnfile|cNfcNfile|co|col|colder|colo|colorscheme|com|comc|comclear|command|compiler|con|conf|confirm|continue|cop|copy|cpf|cpfile|cq|cquit|cr|crewind|cscope|cst|cstag|cu|cuna|cunabbrev|cunmap|cw|cwindow)\b",
                "Statement", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r#"^\s*".*$"#, "Comment", None));
        }
        "toml" => {
            specs.push((
                r"\b(true|false|inf|nan)\b",
                "Boolean", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r"#.*$", "Comment", None));
        }
        "yaml" => {
            specs.push((
                r"\b(true|false|yes|no|on|off|null|~)\b",
                "Boolean", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r"#.*$", "Comment", None));
        }
        "make" | "dockerfile" | "gitconfig" => {
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r"#.*$", "Comment", None));
        }
        "html" => {
            specs.push((r"<!--.*?-->", "Comment", None));
            specs.push((dq_string, "String", None));
        }
        "markdown" => {
            specs.push((r"^#{1,6}\s.*$", "Title", None));
            specs.push((r"`[^`]*`", "String", None));
            specs.push((r"\*\*[^*]+\*\*", "Special", None));
        }
        "tex" => {
            specs.push((r"%.*$", "Comment", None));
            specs.push((r"\\[A-Za-z]+", "Keyword", None));
        }
        "json" => {
            specs.push((
                r"\b(true|false|null)\b",
                "Boolean", None,
            ));
            specs.push((dq_string, "String", None));
            specs.push((number, "Number", None));
        }
        _ => {
            specs.push((dq_string, "String", None));
            specs.push((sq_string, "String", None));
            specs.push((number, "Number", None));
            specs.push((r"//.*$", "Comment", None));
            specs.push((r"#.*$", "Comment", None));
        }
    }
    specs
        .into_iter()
        .filter_map(|(pat, g, cap)| {
            Regex::new(pat).ok().map(|r| Rule {
                regex: r,
                group: g.to_string(),
                capture: cap,
            })
        })
        .collect()
}

// ---- Locating the system syntax file ---------------------------------------

fn find_vim_syntax_file(filetype: &str) -> Option<PathBuf> {
    if filetype == "generic" {
        return None;
    }
    let filename = format!("{filetype}.vim");
    // Local override first.
    let local = PathBuf::from("./syntax").join(&filename);
    if local.exists() {
        return Some(local);
    }
    if let Ok(home) = std::env::var("HOME") {
        let user = PathBuf::from(home).join(".config/rtdvi/syntax").join(&filename);
        if user.exists() {
            return Some(user);
        }
    }
    if let Ok(entries) = std::fs::read_dir("/usr/share/vim") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("vim") && s != "vimfiles" {
                let p = entry.path().join("syntax").join(&filename);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

// ---- Vim syntax-file parsing (subset) --------------------------------------

/// Parse `syn keyword <group> word1 word2 ...` and `hi link <from> <to>`
/// lines out of `text`. Returns compiled keyword regexes mapped to the
/// resolved target group (after following `hi link` chains).
pub fn compile_vim_keywords(text: &str) -> Vec<(Regex, String)> {
    let mut keywords: HashMap<String, Vec<String>> = HashMap::new();
    let mut links: HashMap<String, String> = HashMap::new();

    for raw in text.lines() {
        let line = raw.trim_start();
        if line.starts_with('"') || line.is_empty() {
            continue;
        }
        // `syn keyword GROUP word word ...` (also `:syn`, `:syntax`).
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = matches_prefix(line, &lower, &["syntax keyword", "syn keyword"]) {
            parse_syn_keyword(rest, &mut keywords);
            continue;
        }
        // `hi [def] link FROM TO` / `highlight ...`.
        if lower.starts_with("hi ") || lower.starts_with("highlight ") || lower.starts_with("hi! ") {
            let body = line
                .splitn(2, char::is_whitespace)
                .nth(1)
                .unwrap_or("")
                .trim_start();
            let body = body
                .strip_prefix("def ")
                .or_else(|| body.strip_prefix("default "))
                .unwrap_or(body);
            if let Some(after) = body.strip_prefix("link ") {
                let mut it = after.split_whitespace();
                if let (Some(from), Some(to)) = (it.next(), it.next()) {
                    links.insert(from.to_string(), to.to_string());
                }
            }
        }
    }

    // Resolve each keyword group's *final* target by following `hi link`.
    let resolve = |start: &str| -> String {
        let mut cur = start.to_string();
        for _ in 0..16 {
            match links.get(&cur) {
                Some(next) => cur = next.clone(),
                None => break,
            }
        }
        cur
    };

    let mut out = Vec::new();
    for (group, words) in keywords {
        if words.is_empty() {
            continue;
        }
        // Build a single alternation regex with word boundaries.
        let mut pat = String::from(r"\b(?:");
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                pat.push('|');
            }
            pat.push_str(&regex::escape(w));
        }
        pat.push_str(r")\b");
        if let Ok(re) = Regex::new(&pat) {
            out.push((re, resolve(&group)));
        }
    }
    out
}

fn matches_prefix<'a>(line: &'a str, lower: &str, prefixes: &[&str]) -> Option<&'a str> {
    for p in prefixes {
        if lower.starts_with(p) {
            let after = &line[p.len()..];
            if after.starts_with(char::is_whitespace) {
                return Some(after.trim_start());
            }
        }
    }
    None
}

fn parse_syn_keyword(rest: &str, keywords: &mut HashMap<String, Vec<String>>) {
    // Format: `GROUP word1 word2 ... [contained] [nextgroup=...] [skipwhite] ...`
    let mut it = rest.split_whitespace();
    let Some(group) = it.next() else { return };
    for tok in it {
        // Skip vim's option flags. They contain `=` or are bare keywords.
        if tok.contains('=')
            || matches!(
                tok,
                "contained"
                    | "containedin"
                    | "skipwhite"
                    | "skipempty"
                    | "skipnl"
                    | "transparent"
                    | "display"
                    | "fold"
                    | "extend"
                    | "concealends"
            )
        {
            continue;
        }
        // Ignore `\<`, `\>`, escape sequences etc. — keep it to identifier-ish.
        if tok.chars().all(|c| c.is_alphanumeric() || c == '_') {
            keywords.entry(group.to_string()).or_default().push(tok.to_string());
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
