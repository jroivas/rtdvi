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

/// Mapping from filename / extension → vim filetype string (the basename of
/// the syntax file). Covers the families I personally hit; everything else
/// falls back to `generic`.
pub fn detect_filetype(path: &Path) -> &'static str {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        match name {
            "Makefile" | "makefile" | "GNUmakefile" => return "make",
            "Dockerfile" => return "dockerfile",
            "Cargo.toml" | "Cargo.lock" => return "toml",
            ".gitignore" | ".gitconfig" => return "gitconfig",
            _ => {}
        }
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => "rust",
        "py" => "python",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
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
        _ => "generic",
    }
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
    pub fn for_path(path: Option<&Path>) -> Self {
        let filetype = path.map(detect_filetype).unwrap_or("generic");
        let mut syn = Self {
            filetype,
            rules: builtin_rules(filetype),
            keyword_regexes: Vec::new(),
        };
        // Best-effort: pull keyword/link directives from the system syntax
        // file. Failure is silent — the builtin layer still works.
        if let Some(path) = find_vim_syntax_file(filetype) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                syn.keyword_regexes = compile_vim_keywords(&text);
            }
        }
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
