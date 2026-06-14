//! Built-in per-filetype regex rules, plus parsing of system vim `syn keyword`
//! definitions into additional keyword rules.

use std::collections::HashMap;
use std::path::PathBuf;

use regex::Regex;

use super::Rule;

// ---- Built-in regex rules per filetype ------------------------------------

pub(super) fn builtin_rules(filetype: &str) -> Vec<Rule> {
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

pub(super) fn find_vim_syntax_file(filetype: &str) -> Option<PathBuf> {
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
