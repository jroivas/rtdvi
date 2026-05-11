//! Syntax highlighting.
//!
//! Two layers feed the highlighter:
//!  1. A small built-in *generic* regex layer with patterns for comments,
//!     strings, and numbers per language. Always applied.
//!  2. The vim `syn keyword` definitions parsed out of
//!     `/usr/share/vim/vim*/syntax/<lang>.vim` (and similar lookup paths).
//!     This gives us free keyword highlighting for hundreds of languages
//!     without writing per-language tables ourselves.
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

/// Canonicalise a filetype string into the form jvim/vim uses internally.
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

/// Translate a MIME type (`text/x-rust`) into a filetype jvim understands.
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

/// Compiled rules for one filetype. Pattern order = priority (later wins).
pub struct Syntax {
    pub filetype: &'static str,
    /// Each rule is `(regex, group_name)`. `group_name` is what the colour
    /// scheme looks up via [`Colorscheme::style_for`].
    pub rules: Vec<(Regex, String)>,
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
        // straight to a `.vim` file (e.g. ones jvim's builtin map misses)
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
        for (prio, (regex, group)) in self.rules.iter().enumerate() {
            for m in regex.find_iter(line) {
                paint(&mut per_byte, m.start(), m.end(), (prio + 1) as u32, group);
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
}

fn paint<'a>(buf: &mut [Option<(u32, &'a str)>], start: usize, end: usize, prio: u32, group: &'a str) {
    for i in start..end.min(buf.len()) {
        if buf[i].map_or(true, |(p, _)| prio >= p) {
            buf[i] = Some((prio, group));
        }
    }
}

// ---- Built-in regex rules per filetype ------------------------------------

fn builtin_rules(filetype: &str) -> Vec<(Regex, String)> {
    let mut rules: Vec<(&str, &str)> = Vec::new();
    // String rule shared by most languages.
    let dq_string = r#""(?:\\.|[^"\\])*""#;
    let sq_string = r#"'(?:\\.|[^'\\])*'"#;
    let number = r"\b\d+(?:\.\d+)?\b";
    match filetype {
        "rust" | "c" | "cpp" | "go" | "java" | "javascript" | "typescript" | "css" => {
            rules.push((dq_string, "String"));
            rules.push((number, "Number"));
            rules.push((r"//.*$", "Comment"));
            rules.push((r"/\*.*?\*/", "Comment"));
        }
        "python" | "sh" | "ruby" | "toml" | "yaml" | "make" | "dockerfile" | "gitconfig" => {
            rules.push((dq_string, "String"));
            rules.push((sq_string, "String"));
            rules.push((number, "Number"));
            rules.push((r"#.*$", "Comment"));
        }
        "vim" => {
            rules.push((dq_string, "String"));
            rules.push((sq_string, "String"));
            rules.push((number, "Number"));
            // Vim comments start with `"` at line start; conservative.
            rules.push((r#"^\s*".*$"#, "Comment"));
        }
        "lua" => {
            rules.push((dq_string, "String"));
            rules.push((sq_string, "String"));
            rules.push((number, "Number"));
            rules.push((r"--.*$", "Comment"));
        }
        "html" => {
            rules.push((r"<!--.*?-->", "Comment"));
            rules.push((dq_string, "String"));
        }
        "markdown" => {
            rules.push((r"^#{1,6}\s.*$", "Title"));
            rules.push((r"`[^`]*`", "String"));
            rules.push((r"\*\*[^*]+\*\*", "Special"));
        }
        "tex" => {
            rules.push((r"%.*$", "Comment"));
            rules.push((r"\\[A-Za-z]+", "Keyword"));
        }
        "json" => {
            rules.push((dq_string, "String"));
            rules.push((number, "Number"));
        }
        _ => {
            // Generic: best-effort guesses.
            rules.push((dq_string, "String"));
            rules.push((sq_string, "String"));
            rules.push((number, "Number"));
            rules.push((r"//.*$", "Comment"));
            rules.push((r"#.*$", "Comment"));
        }
    }
    rules
        .into_iter()
        .filter_map(|(pat, g)| Regex::new(pat).ok().map(|r| (r, g.to_string())))
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
        let user = PathBuf::from(home).join(".config/jvim/syntax").join(&filename);
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
    fn parse_vim_keywords_handles_options() {
        let kws =
            compile_vim_keywords("syn keyword rustKeyword pub use mod nextgroup=foo skipwhite\nhi def link rustKeyword Keyword");
        assert_eq!(kws.len(), 1);
        let (_, g) = &kws[0];
        assert_eq!(g, "Keyword");
    }
}
