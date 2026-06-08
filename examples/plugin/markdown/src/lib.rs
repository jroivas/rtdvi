//! markdown — a render-buffer plugin for rtdvi.
//!
//! Registers the `:md` command. Running it parses the **current buffer** as
//! markdown and opens a non-editable, styled split (a "render buffer") via
//! rtdvi's render ABI. Links are followable in the view: put the cursor on a
//! `[text](other.md)` link (or jump with `<Tab>`) and press `<Enter>` to render
//! the linked page in the same pane. `q` closes the view.
//!
//! Supported: ATX (`#`) and setext (`===`/`---`) headings, **bold**, *italic*,
//! `code`, ~~strikethrough~~, fenced code blocks, ordered / unordered / nested
//! lists, task lists (`- [x]`), block quotes (incl. nesting), horizontal rules,
//! tables (with column alignment), images, links — inline `[text](url)`,
//! reference `[text][ref]` / `[ref]` (with `[ref]: url` definitions), `<url>`,
//! and bare URLs — and backslash escapes.
//!
//! ## Build
//!
//!   rustup target add wasm32-unknown-unknown
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/markdown.wasm ~/.local/rtdvi/plugins/
//!
//! ## Enable in ~/.config/rtdvi/config.toml
//!
//!   plugins = ["markdown"]
//!
//! `parse_markdown` is plain Rust, unit-tested on the host; the WASM ABI glue
//! is gated to `target_arch = "wasm32"`.

// ── Styled-content model (host-agnostic) ──────────────────────────────────────

/// A styled text segment. `fg` is `0xRRGGBB`; `size` is an advisory heading
/// scale (0 = body, 1..=6 = h1..h6); `link` makes it a hyperlink.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub fg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub size: u8,
    pub link: Option<String>,
}

/// One rendered line = a sequence of styled segments.
pub type RLine = Vec<Span>;

/// Link reference definitions (`[label]: url`), keyed by lowercased label.
pub type Refs = std::collections::HashMap<String, String>;

// Nord-ish palette.
const FG_H: [u32; 6] = [0x88C0D0, 0x81A1C1, 0x8FBCBB, 0xA3BE8C, 0xA3BE8C, 0xA3BE8C];
const FG_CODE: u32 = 0xEBCB8B;
const FG_LINK: u32 = 0x8FBCBB;
const FG_IMG: u32 = 0xB48EAD;
const FG_QUOTE: u32 = 0x616E88;
const FG_RULE: u32 = 0x4C566A;
const FG_BULLET: u32 = 0xB48EAD;
const FG_CHECK: u32 = 0xA3BE8C;

fn plain(text: impl Into<String>) -> Span {
    Span { text: text.into(), ..Default::default() }
}

fn colored(text: impl Into<String>, fg: u32) -> Span {
    Span { text: text.into(), fg: Some(fg), ..Default::default() }
}

fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.text.chars().count()).sum()
}

// ── Markdown → styled lines (pure) ────────────────────────────────────────────

/// Parse markdown source lines into styled render lines.
pub fn parse_markdown(lines: &[String]) -> Vec<RLine> {
    let (refs, defs) = collect_refs(lines);
    let mut out: Vec<RLine> = Vec::new();
    let mut in_fence = false;
    let mut i = 0;

    while i < lines.len() {
        let raw = &lines[i];
        let trimmed = raw.trim_start();

        // Fenced code blocks: toggle on ``` and render contents verbatim.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push(vec![colored(raw.clone(), FG_RULE)]);
            i += 1;
            continue;
        }
        if in_fence {
            out.push(vec![colored(raw.clone(), FG_CODE)]);
            i += 1;
            continue;
        }

        // Link reference definitions (`[label]: url`) are not rendered.
        if defs.contains(&i) {
            i += 1;
            continue;
        }

        // Tables: a row followed by a delimiter row (`|---|:--:|`).
        if raw.contains('|') && is_table_delimiter(lines.get(i + 1)) {
            let (consumed, rows) = parse_table(&lines[i..], &refs);
            out.extend(rows);
            i += consumed;
            continue;
        }

        // Setext headings: a paragraph line underlined by `===` (h1) / `---` (h2).
        if is_paragraph_candidate(trimmed) {
            if let Some(level) = setext_level(lines.get(i + 1)) {
                out.push(style_heading(parse_inline_with(trimmed, &refs), level));
                i += 2;
                continue;
            }
        }

        // Horizontal rule.
        if is_hr(trimmed) {
            out.push(vec![colored("─".repeat(40), FG_RULE)]);
            i += 1;
            continue;
        }

        // ATX heading.
        if let Some(level) = heading_level(trimmed) {
            let text = trimmed[level..].trim_start().trim_end_matches('#').trim_end();
            out.push(style_heading(parse_inline_with(text, &refs), level));
            i += 1;
            continue;
        }

        // Blockquote (possibly nested: `>`, `>>`, `> >`).
        if trimmed.starts_with('>') {
            let (depth, rest) = quote_depth(trimmed);
            let mut line = vec![colored("▌".repeat(depth) + " ", FG_QUOTE)];
            for mut s in parse_inline_with(rest, &refs) {
                s.italic = true;
                if s.fg.is_none() {
                    s.fg = Some(FG_QUOTE);
                }
                line.push(s);
            }
            out.push(line);
            i += 1;
            continue;
        }

        // List items (unordered / ordered / task), preserving nesting indent.
        if let Some(line) = parse_list_item(raw, &refs) {
            out.push(line);
            i += 1;
            continue;
        }

        // Plain paragraph (possibly empty) with inline styling.
        out.push(parse_inline_with(raw, &refs));
        i += 1;
    }
    out
}

/// Pre-scan for link reference definitions (`[label]: url ["title"]`),
/// returning the label→url map (labels lowercased) and the set of line indices
/// that are definitions (so they can be dropped from the rendered output).
/// Definitions inside fenced code blocks are ignored.
fn collect_refs(lines: &[String]) -> (Refs, std::collections::HashSet<usize>) {
    let mut refs = Refs::new();
    let mut defs = std::collections::HashSet::new();
    let mut in_fence = false;
    for (idx, raw) in lines.iter().enumerate() {
        let t = raw.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((label, url)) = parse_link_def(raw) {
            refs.insert(label, url);
            defs.insert(idx);
        }
    }
    (refs, defs)
}

/// `[label]: url ["title"]` → (lowercased label, url). `[^foot]:` footnote
/// definitions are skipped.
fn parse_link_def(raw: &str) -> Option<(String, String)> {
    let rest = raw.trim_start().strip_prefix('[')?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() || label.starts_with('^') {
        return None;
    }
    let after = rest[close + 2..].trim();
    let url = after.split_whitespace().next()?;
    let url = url.trim_start_matches('<').trim_end_matches('>');
    if url.is_empty() {
        return None;
    }
    Some((label.to_lowercase(), url.to_string()))
}

/// `# ` … `###### ` → heading level 1..6, else None.
fn heading_level(s: &str) -> Option<usize> {
    let hashes = s.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) && s.as_bytes().get(hashes) == Some(&b' ') {
        Some(hashes)
    } else {
        None
    }
}

/// Apply heading style (colour + bold + advisory size) to already-parsed inline
/// spans, so links inside a heading (`# [Foo](bar)`) keep working.
fn style_heading(mut spans: RLine, level: usize) -> RLine {
    for s in &mut spans {
        s.bold = true;
        s.size = level as u8;
        if s.fg.is_none() {
            s.fg = Some(FG_H[level - 1]);
        }
    }
    spans
}

/// A setext underline: a line of only `=` (h1) or only `-` (h2).
fn setext_level(next: Option<&String>) -> Option<usize> {
    let t = next?.trim();
    if t.is_empty() {
        return None;
    }
    if t.bytes().all(|b| b == b'=') {
        Some(1)
    } else if t.bytes().all(|b| b == b'-') {
        Some(2)
    } else {
        None
    }
}

/// True when `t` could be a paragraph that a setext underline applies to —
/// i.e. not a heading/quote/list/table/fence/blank line.
fn is_paragraph_candidate(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    !(t.starts_with('#')
        || t.starts_with('>')
        || t.starts_with('|')
        || t.starts_with("```")
        || starts_list_marker(t))
}

/// True if `t` (already left-trimmed) begins with a list marker.
fn starts_list_marker(t: &str) -> bool {
    t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") || ordered_item(t).is_some()
}

/// A horizontal rule: 3+ of `-`/`*`/`_` after removing spaces.
fn is_hr(t: &str) -> bool {
    let s: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3 && (s.bytes().all(|b| b == b'-') || s.bytes().all(|b| b == b'*') || s.bytes().all(|b| b == b'_'))
}

/// Count leading `>` markers (allowing spaces between) and return the depth +
/// the remaining text.
fn quote_depth(s: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = s;
    while let Some(r) = rest.strip_prefix('>') {
        depth += 1;
        rest = r.trim_start();
    }
    (depth, rest)
}

/// Parse a list item line (unordered `-`/`*`/`+`, ordered `N.`, optionally a
/// task `[x]`/`[ ]`), preserving the indentation depth. Returns `None` if `raw`
/// is not a list item.
fn parse_list_item(raw: &str, refs: &Refs) -> Option<RLine> {
    let indent = raw.len() - raw.trim_start().len();
    let body = &raw[indent..];

    let (marker_len, ordered) = if let Some(rest) = body
        .strip_prefix("- ")
        .or_else(|| body.strip_prefix("* "))
        .or_else(|| body.strip_prefix("+ "))
    {
        (body.len() - rest.len(), None)
    } else if let Some((num, _rest)) = ordered_item(body) {
        (num.len() + 2, Some(num))
    } else {
        return None;
    };
    let content = &body[marker_len..];

    // Indent reflects the source nesting only — top-level items sit at the left
    // margin (no extra padding), nested items keep their relative indent.
    let pad = " ".repeat(indent);
    let mut line: RLine = Vec::new();

    // Task list checkbox.
    if let Some(after) = content
        .strip_prefix("[ ] ")
        .map(|r| ("☐ ", FG_BULLET, r))
        .or_else(|| content.strip_prefix("[x] ").map(|r| ("☑ ", FG_CHECK, r)))
        .or_else(|| content.strip_prefix("[X] ").map(|r| ("☑ ", FG_CHECK, r)))
    {
        let (mark, fg, rest) = after;
        line.push(colored(pad, FG_BULLET));
        line.push(colored(mark, fg));
        line.extend(parse_inline_with(rest, refs));
        return Some(line);
    }

    match ordered {
        Some(num) => line.push(colored(format!("{pad}{num}. "), FG_BULLET)),
        None => line.push(colored(format!("{pad}• "), FG_BULLET)),
    }
    line.extend(parse_inline_with(content, refs));
    Some(line)
}

/// `12. rest` → ("12", "rest").
fn ordered_item(s: &str) -> Option<(String, &str)> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = s[digits.len()..].strip_prefix(". ")?;
    Some((digits, rest))
}

// ── Tables ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// True when `line` is a table delimiter row, e.g. `| --- |:--:| ---: |`.
fn is_table_delimiter(line: Option<&String>) -> bool {
    let l = match line {
        Some(l) => l.trim(),
        None => return false,
    };
    if !l.contains('|') && !l.contains('-') {
        return false;
    }
    let cells = split_row(l);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.contains('-') && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Split a table row into trimmed cell strings, honouring `\|` escapes and
/// stripping the optional outer pipes.
fn split_row(line: &str) -> Vec<String> {
    let l = line.trim();
    let l = l.strip_prefix('|').unwrap_or(l);
    let l = l.strip_suffix('|').unwrap_or(l);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = l.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&n) = chars.peek() {
                cur.push(n);
                chars.next();
                continue;
            }
        }
        if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn alignment(delim: &str) -> Align {
    let d = delim.trim();
    let l = d.starts_with(':');
    let r = d.ends_with(':');
    match (l, r) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

/// Parse a markdown table starting at `lines[0]` (header) + `lines[1]`
/// (delimiter). Returns (lines consumed, rendered rows).
fn parse_table(lines: &[String], refs: &Refs) -> (usize, Vec<RLine>) {
    let header = split_row(&lines[0]);
    let aligns: Vec<Align> = split_row(&lines[1]).iter().map(|d| alignment(d)).collect();
    let ncol = header.len();

    // Body rows: consecutive lines that still look like rows.
    let mut body: Vec<Vec<String>> = Vec::new();
    let mut consumed = 2;
    while let Some(line) = lines.get(consumed) {
        if line.trim().is_empty() || !line.contains('|') {
            break;
        }
        body.push(split_row(line));
        consumed += 1;
    }

    // Parse every cell's inline spans and compute column widths.
    let cell_spans = |cells: &[String]| -> Vec<RLine> {
        (0..ncol)
            .map(|c| parse_inline_with(cells.get(c).map(|s| s.as_str()).unwrap_or(""), refs))
            .collect()
    };
    let header_cells = cell_spans(&header);
    let body_cells: Vec<Vec<RLine>> = body.iter().map(|r| cell_spans(r)).collect();

    let mut col_w = vec![3usize; ncol];
    for c in 0..ncol {
        col_w[c] = col_w[c].max(width(&header_cells[c]));
        for row in &body_cells {
            col_w[c] = col_w[c].max(width(&row[c]));
        }
    }

    let align_of = |c: usize| aligns.get(c).copied().unwrap_or(Align::Left);
    let mut out: Vec<RLine> = Vec::new();
    out.push(table_row(&header_cells, &col_w, |c| align_of(c), true));
    out.push(vec![colored(separator(&col_w), FG_RULE)]);
    for row in &body_cells {
        out.push(table_row(row, &col_w, |c| align_of(c), false));
    }
    (consumed, out)
}

fn table_row(cells: &[RLine], col_w: &[usize], align: impl Fn(usize) -> Align, header: bool) -> RLine {
    let mut out: RLine = vec![colored("│", FG_RULE)];
    for (c, w) in col_w.iter().enumerate() {
        let empty: RLine = Vec::new();
        let cell = cells.get(c).unwrap_or(&empty);
        let pad = w.saturating_sub(width(cell));
        let (lp, rp) = match align(c) {
            Align::Right => (pad, 0),
            Align::Center => (pad / 2, pad - pad / 2),
            Align::Left => (0, pad),
        };
        out.push(plain(" "));
        if lp > 0 {
            out.push(plain(" ".repeat(lp)));
        }
        for s in cell {
            let mut s = s.clone();
            if header {
                s.bold = true;
            }
            out.push(s);
        }
        if rp > 0 {
            out.push(plain(" ".repeat(rp)));
        }
        out.push(colored(" │", FG_RULE));
    }
    out
}

fn separator(col_w: &[usize]) -> String {
    let mut s = String::from("├");
    for (c, w) in col_w.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2));
        s.push(if c + 1 < col_w.len() { '┼' } else { '┤' });
    }
    s
}

// ── Inline parsing ─────────────────────────────────────────────────────────────

/// Inline markdown: backslash escapes, `` `code` ``, `![img](url)`,
/// `[text](url)`, `<url>` and bare URL autolinks, `~~strike~~`,
/// `**bold**`/`__bold__`, `*italic*`/`_italic_`. Plain runs are copied verbatim
/// (every marker is ASCII, so byte slicing on run boundaries is sound).
pub fn parse_inline(s: &str) -> RLine {
    parse_inline_with(s, &Refs::new())
}

/// Inline parsing with link reference definitions available for `[text][ref]`,
/// `[text][]`, and shortcut `[ref]` resolution.
pub fn parse_inline_with(s: &str, refs: &Refs) -> RLine {
    let mut out: RLine = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let mut run = 0;

    let flush = |run: usize, i: usize, out: &mut RLine| {
        if run < i {
            out.push(plain(&s[run..i]));
        }
    };

    while i < b.len() {
        // Backslash escape: `\X` → literal X (for ASCII punctuation).
        if b[i] == b'\\' && i + 1 < b.len() && b[i + 1].is_ascii_punctuation() {
            flush(run, i, &mut out);
            out.push(plain(&s[i + 1..i + 2]));
            i += 2;
            run = i;
            continue;
        }
        // `code`
        if b[i] == b'`' {
            if let Some(end) = s[i + 1..].find('`') {
                let end = i + 1 + end;
                flush(run, i, &mut out);
                out.push(colored(&s[i + 1..end], FG_CODE));
                i = end + 1;
                run = i;
                continue;
            }
        }
        // ![alt](url) / ![alt][ref] image — render as a labelled placeholder.
        if b[i] == b'!' && b.get(i + 1) == Some(&b'[') {
            let img = link_at(s, i + 1)
                .map(|(alt, _url, next)| (alt, next))
                .or_else(|| {
                    ref_link_at(s, i + 1).and_then(|(alt, label, next)| {
                        refs.get(&label.to_lowercase()).map(|_| (alt, next))
                    })
                });
            if let Some((alt, next)) = img {
                flush(run, i, &mut out);
                let label = if alt.is_empty() { "image".to_string() } else { alt };
                out.push(Span { text: format!("🖼 {label}"), fg: Some(FG_IMG), ..Default::default() });
                i = next;
                run = i;
                continue;
            }
        }
        // [text](url) inline link, or [text][ref] / [ref] reference link.
        if b[i] == b'[' {
            let resolved = link_at(s, i).map(|(text, url, next)| (text, url, next)).or_else(|| {
                ref_link_at(s, i).and_then(|(text, label, next)| {
                    refs.get(&label.to_lowercase()).map(|url| (text, url.clone(), next))
                })
            });
            if let Some((text, url, next)) = resolved {
                flush(run, i, &mut out);
                out.push(Span {
                    text,
                    fg: Some(FG_LINK),
                    underline: true,
                    link: Some(url),
                    ..Default::default()
                });
                i = next;
                run = i;
                continue;
            }
        }
        // <url> autolink (other <...> left as literal text, e.g. HTML tags).
        if b[i] == b'<' {
            if let Some(end) = s[i + 1..].find('>') {
                let inner = &s[i + 1..i + 1 + end];
                if is_url(inner) {
                    flush(run, i, &mut out);
                    out.push(Span {
                        text: inner.to_string(),
                        fg: Some(FG_LINK),
                        underline: true,
                        link: Some(inner.to_string()),
                        ..Default::default()
                    });
                    i = i + 1 + end + 1;
                    run = i;
                    continue;
                }
            }
        }
        // Bare URL autolink.
        if (s[i..].starts_with("http://") || s[i..].starts_with("https://")) && at_boundary(b, i) {
            let end = url_end(s, i);
            flush(run, i, &mut out);
            out.push(Span {
                text: s[i..end].to_string(),
                fg: Some(FG_LINK),
                underline: true,
                link: Some(s[i..end].to_string()),
                ..Default::default()
            });
            i = end;
            run = i;
            continue;
        }
        // ~~strikethrough~~
        if b[i] == b'~' && b.get(i + 1) == Some(&b'~') {
            if let Some(end) = s[i + 2..].find("~~") {
                let end = i + 2 + end;
                flush(run, i, &mut out);
                out.push(Span { text: s[i + 2..end].to_string(), strike: true, ..Default::default() });
                i = end + 2;
                run = i;
                continue;
            }
        }
        // **bold** / __bold__
        if i + 1 < b.len() && ((b[i] == b'*' && b[i + 1] == b'*') || (b[i] == b'_' && b[i + 1] == b'_')) {
            let marker = &s[i..i + 2];
            if let Some(end) = s[i + 2..].find(marker) {
                let end = i + 2 + end;
                flush(run, i, &mut out);
                out.push(Span { text: s[i + 2..end].to_string(), bold: true, ..Default::default() });
                i = end + 2;
                run = i;
                continue;
            }
        }
        // *italic* / _italic_
        if b[i] == b'*' || b[i] == b'_' {
            let m = b[i] as char;
            if let Some(end) = s[i + 1..].find(m) {
                let end = i + 1 + end;
                if end > i + 1 {
                    flush(run, i, &mut out);
                    out.push(Span { text: s[i + 1..end].to_string(), italic: true, ..Default::default() });
                    i = end + 1;
                    run = i;
                    continue;
                }
            }
        }
        // Not a marker: advance a whole UTF-8 char so `i` stays on a boundary
        // (the `s[i..]` slices above require it).
        i += utf8_len(b[i]);
    }
    flush(run, b.len(), &mut out);
    if out.is_empty() {
        out.push(plain("")); // preserve blank lines
    }
    out
}

/// Byte length of the UTF-8 character whose leading byte is `b`.
fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

/// Parse a `[text](url ...)` starting at byte `open` (the `[`). Returns
/// (text, url, byte index just past the `)`). The optional `"title"` is dropped.
fn link_at(s: &str, open: usize) -> Option<(String, String, usize)> {
    let b = s.as_bytes();
    if b.get(open) != Some(&b'[') {
        return None;
    }
    let close = open + 1 + s[open + 1..].find(']')?;
    if b.get(close + 1) != Some(&b'(') {
        return None;
    }
    let paren = close + 2 + s[close + 2..].find(')')?;
    let text = s[open + 1..close].to_string();
    // Strip an optional `"title"` from the destination.
    let dest = s[close + 2..paren].trim();
    let url = dest.split_whitespace().next().unwrap_or("").to_string();
    Some((text, url, paren + 1))
}

/// Parse a reference link starting at `open` (the `[`): `[text][label]`,
/// `[text][]` (implicit label = text), or shortcut `[text]`. Returns
/// (text, label, byte index past the close). The caller looks `label` up in the
/// reference map; if absent, it should fall through to literal text.
fn ref_link_at(s: &str, open: usize) -> Option<(String, String, usize)> {
    let b = s.as_bytes();
    if b.get(open) != Some(&b'[') {
        return None;
    }
    let close = open + 1 + s[open + 1..].find(']')?;
    let text = s[open + 1..close].to_string();
    if b.get(close + 1) == Some(&b'[') {
        let close2 = close + 2 + s[close + 2..].find(']')?;
        let label = s[close + 2..close2].to_string();
        let label = if label.is_empty() { text.clone() } else { label };
        return Some((text, label, close2 + 1));
    }
    // Shortcut reference: label = text.
    Some((text.clone(), text, close + 1))
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("ftp://") || s.starts_with("mailto:")
}

/// A bare URL must start at a word boundary (start of string or after space/`(`).
fn at_boundary(b: &[u8], i: usize) -> bool {
    i == 0 || matches!(b[i - 1], b' ' | b'\t' | b'(' | b'<')
}

/// End of a bare URL: stop at whitespace or a closing delimiter, and don't
/// swallow trailing sentence punctuation.
fn url_end(s: &str, start: usize) -> usize {
    let bytes = s.as_bytes();
    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b' ' | b'\t' | b'<' | b'>' | b'"' | b')' | b']') {
        end += 1;
    }
    while end > start && matches!(bytes[end - 1], b'.' | b',' | b';' | b':' | b'!' | b'?') {
        end -= 1;
    }
    end
}

// ── WASM ABI glue (only compiled for the plugin target) ───────────────────────

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::alloc::Layout;
    use std::slice;

    #[link(wasm_import_module = "rtdvi")]
    extern "C" {
        fn rtdvi_register_command(ptr: i32, len: i32) -> i32;
        fn rtdvi_set_status(ptr: i32, len: i32);
        fn rtdvi_active_buffer_id() -> i32;
        fn rtdvi_line_count(buf_id: i32) -> i32;
        fn rtdvi_get_line(buf_id: i32, row: i32, out_ptr: i32, max_len: i32) -> i32;
        fn rtdvi_render_span(
            text_ptr: i32, text_len: i32,
            fg: i32, bg: i32, attrs: i32, size: i32,
            target_ptr: i32, target_len: i32,
        ) -> i32;
        fn rtdvi_render_newline();
        fn rtdvi_render_open(title_ptr: i32, title_len: i32) -> i32;
    }

    #[no_mangle]
    pub extern "C" fn alloc(size: i32) -> i32 {
        if size <= 0 {
            return 0;
        }
        match Layout::from_size_align(size as usize, 8) {
            Ok(l) => unsafe { std::alloc::alloc(l) as i32 },
            Err(_) => 0,
        }
    }

    #[no_mangle]
    pub extern "C" fn dealloc(ptr: i32, size: i32) {
        if ptr == 0 || size <= 0 {
            return;
        }
        if let Ok(l) = Layout::from_size_align(size as usize, 8) {
            unsafe { std::alloc::dealloc(ptr as *mut u8, l) }
        }
    }

    fn get_line(buf_id: i32, row: i32) -> String {
        let mut buf = vec![0u8; 8192];
        let n = unsafe { rtdvi_get_line(buf_id, row, buf.as_mut_ptr() as i32, buf.len() as i32) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    }

    fn emit_span(s: &Span) {
        let fg = s.fg.map(|c| c as i32).unwrap_or(-1);
        let mut attrs = 0;
        if s.bold {
            attrs |= 1;
        }
        if s.italic {
            attrs |= 2;
        }
        if s.underline {
            attrs |= 4;
        }
        if s.strike {
            attrs |= 16;
        }
        let (tptr, tlen) = match &s.link {
            Some(t) => (t.as_ptr() as i32, t.len() as i32),
            None => (0, 0),
        };
        unsafe {
            rtdvi_render_span(
                s.text.as_ptr() as i32, s.text.len() as i32,
                fg, -1, attrs, s.size as i32,
                tptr, tlen,
            );
        }
    }

    #[no_mangle]
    pub extern "C" fn rtdvi_init(_cfg_ptr: i32, _cfg_len: i32) -> i32 {
        let name = "md";
        unsafe { rtdvi_register_command(name.as_ptr() as i32, name.len() as i32) };
        0
    }

    #[no_mangle]
    pub extern "C" fn run_command(name_ptr: i32, name_len: i32, _a: i32, _b: i32) -> i32 {
        let name = unsafe {
            let bytes = slice::from_raw_parts(name_ptr as *const u8, name_len as usize);
            std::str::from_utf8(bytes).unwrap_or("")
        };
        if name != "md" {
            return -1;
        }
        let buf = unsafe { rtdvi_active_buffer_id() };
        if buf < 0 {
            let m = "md: no active buffer";
            unsafe { rtdvi_set_status(m.as_ptr() as i32, m.len() as i32) };
            return 0;
        }
        let n = unsafe { rtdvi_line_count(buf) }.max(0);
        let lines: Vec<String> = (0..n).map(|r| get_line(buf, r)).collect();
        for line in parse_markdown(&lines) {
            for span in &line {
                emit_span(span);
            }
            unsafe { rtdvi_render_newline() };
        }
        let title = "[Markdown]";
        unsafe { rtdvi_render_open(title.as_ptr() as i32, title.len() as i32) };
        0
    }
}

// ── Tests (host) ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn joined(line: &RLine) -> String {
        line.iter().map(|sp| sp.text.as_str()).collect()
    }

    #[test]
    fn atx_headings_carry_size_and_bold() {
        let out = parse_markdown(&s(&["# Title", "### Sub"]));
        assert_eq!(out[0][0].text, "Title");
        assert_eq!(out[0][0].size, 1);
        assert!(out[0][0].bold);
        assert_eq!(out[1][0].size, 3);
    }

    #[test]
    fn setext_headings() {
        let out = parse_markdown(&s(&["Alt-H1", "======", "Alt-H2", "------"]));
        assert_eq!(out.len(), 2, "two lines consumed as headings");
        assert_eq!((out[0][0].text.as_str(), out[0][0].size), ("Alt-H1", 1));
        assert_eq!((out[1][0].text.as_str(), out[1][0].size), ("Alt-H2", 2));
    }

    #[test]
    fn standalone_dashes_are_a_rule_not_a_heading() {
        let out = parse_markdown(&s(&["", "------", ""]));
        assert!(out[1][0].text.starts_with('─'));
    }

    #[test]
    fn heading_with_link() {
        let out = parse_markdown(&s(&["# [Footnotes](https://x)"]));
        let link = out[0].iter().find(|sp| sp.link.is_some()).unwrap();
        assert_eq!(link.text, "Footnotes");
        assert!(link.bold && link.size == 1);
    }

    #[test]
    fn inline_bold_italic_code_strike() {
        let line = parse_inline("a **b** *d* `e` ~~f~~");
        assert!(line.iter().any(|sp| sp.text == "b" && sp.bold));
        assert!(line.iter().any(|sp| sp.text == "d" && sp.italic));
        assert!(line.iter().any(|sp| sp.text == "e" && sp.fg == Some(FG_CODE)));
        assert!(line.iter().any(|sp| sp.text == "f" && sp.strike));
    }

    #[test]
    fn backslash_escape_disables_markdown() {
        let line = parse_inline(r"\*not italic\*");
        assert_eq!(joined(&line), "*not italic*");
        assert!(line.iter().all(|sp| !sp.italic));
    }

    #[test]
    fn links_inline_angle_and_bare() {
        let a = parse_inline("[t](u.md)");
        assert_eq!(a[0].link.as_deref(), Some("u.md"));
        let b = parse_inline("<http://example.com>");
        assert_eq!(b[0].link.as_deref(), Some("http://example.com"));
        let c = parse_inline("see http://example.com now");
        assert!(c.iter().any(|sp| sp.link.as_deref() == Some("http://example.com")));
    }

    #[test]
    fn link_title_is_stripped() {
        let a = parse_inline(r#"[t](http://x "Title")"#);
        assert_eq!(a[0].link.as_deref(), Some("http://x"));
    }

    #[test]
    fn image_renders_as_placeholder() {
        let line = parse_inline("![alt](pic.png)");
        assert!(line[0].text.contains("alt"));
        assert!(line[0].link.is_none(), "image is not a followable link");
    }

    #[test]
    fn task_list_checkboxes() {
        let out = parse_markdown(&s(&["- [x] done", "- [ ] todo"]));
        assert!(out[0].iter().any(|sp| sp.text.contains('☑')));
        assert!(out[1].iter().any(|sp| sp.text.contains('☐')));
    }

    #[test]
    fn nested_list_indentation() {
        let out = parse_markdown(&s(&["- top", "    - nested"]));
        // The nested item is indented further than the top-level one.
        let top_pad = out[0][0].text.len();
        let nested_pad = out[1][0].text.len();
        assert!(nested_pad > top_pad, "nested item should be more indented");
    }

    #[test]
    fn top_level_list_items_are_not_indented() {
        let out = parse_markdown(&s(&["1. First", "2. Another"]));
        // Marker starts at column 0 — no spurious leading padding.
        assert_eq!(out[0][0].text, "1. ");
        assert_eq!(out[1][0].text, "2. ");
    }

    #[test]
    fn reference_style_links_resolve() {
        let out = parse_markdown(&s(&[
            "[case ref][Arbitrary Ref]",
            "[numbered][1]",
            "and the [shortcut] works",
            "",
            "[arbitrary ref]: https://moz.org",
            "[1]: http://slashdot.org",
            "[shortcut]: http://reddit.com",
        ]));
        // The three definition lines are dropped; 4 content lines remain
        // (incl. the blank line).
        let links: Vec<_> = out
            .iter()
            .flatten()
            .filter_map(|sp| sp.link.clone())
            .collect();
        assert!(links.contains(&"https://moz.org".to_string()));
        assert!(links.contains(&"http://slashdot.org".to_string()));
        assert!(links.contains(&"http://reddit.com".to_string()));
        // Definition lines are not rendered as literal text.
        let all: String = out.iter().flatten().map(|sp| sp.text.clone()).collect();
        assert!(!all.contains("]: http"), "definition lines should be dropped");
    }

    #[test]
    fn unknown_reference_stays_literal() {
        let out = parse_markdown(&s(&["see [missing] ref"]));
        let all: String = out.iter().flatten().map(|sp| sp.text.clone()).collect();
        assert!(all.contains("[missing]"));
        assert!(out.iter().flatten().all(|sp| sp.link.is_none()));
    }

    #[test]
    fn nested_blockquote_depth() {
        let out = parse_markdown(&s(&["> a", ">> b"]));
        let d1 = out[0][0].text.matches('▌').count();
        let d2 = out[1][0].text.matches('▌').count();
        assert_eq!((d1, d2), (1, 2));
    }

    #[test]
    fn fenced_code_is_verbatim() {
        let out = parse_markdown(&s(&["```", "let x = 1; // **not bold**", "```"]));
        assert_eq!(out[1].len(), 1);
        assert_eq!(out[1][0].text, "let x = 1; // **not bold**");
        assert_eq!(out[1][0].fg, Some(FG_CODE));
    }

    #[test]
    fn table_renders_header_separator_and_rows() {
        let out = parse_markdown(&s(&[
            "| A | B |",
            "| --- | ---: |",
            "| 1 | 2 |",
        ]));
        assert_eq!(out.len(), 3, "header + separator + 1 body row");
        assert!(joined(&out[0]).contains('A') && joined(&out[0]).contains('B'));
        assert!(joined(&out[1]).contains('─'), "separator row");
        assert!(joined(&out[2]).contains('1') && joined(&out[2]).contains('2'));
        // Header cells are bold.
        assert!(out[0].iter().any(|sp| sp.text.contains('A') && sp.bold));
    }

    #[test]
    fn table_cell_inline_and_escaped_pipe() {
        let out = parse_markdown(&s(&[
            "| Name | Char |",
            "| --- | --- |",
            "| `code` | \\| |",
        ]));
        // Inline code inside a cell keeps its style.
        assert!(out[2].iter().any(|sp| sp.text == "code" && sp.fg == Some(FG_CODE)));
        // Escaped pipe is literal content, not a column separator.
        assert!(joined(&out[2]).contains('|'));
    }

    #[test]
    fn parses_the_full_github_corpus_without_panic() {
        // The repo's example.md is the GitHub "all markdown tricks" gist.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../example.md");
        if let Ok(text) = std::fs::read_to_string(path) {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let out = parse_markdown(&lines);
            assert!(out.len() >= lines.len() / 2, "should produce roughly one line each");
        }
    }

    #[test]
    fn utf8_runs_are_preserved() {
        let line = parse_inline("café **ä** ☃");
        assert_eq!(line[0].text, "café ");
        assert!(line.iter().any(|sp| sp.text == "ä" && sp.bold));
    }
}
