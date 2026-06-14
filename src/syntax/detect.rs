//! Filetype detection: extension/MIME/glob rules and built-in mappings.

use std::collections::HashMap;
use std::path::Path;

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
