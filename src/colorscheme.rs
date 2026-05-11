//! Vim-style colorscheme loader.
//!
//! Parses a subset of `:highlight` directives from `.vim` colorscheme files
//! into a map of group name → [`ratatui::style::Style`]. Everything else in
//! the file (let/set/if/syntax/etc.) is ignored.
//!
//! Lookup is by name through [`load`], which walks:
//!   1. `./colors/<name>.vim`
//!   2. `$XDG_CONFIG_HOME/jvim/colors/<name>.vim` (or `~/.config/jvim/...`)
//!   3. `/usr/share/vim/vim*/colors/<name>.vim`

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::style::{Color, Modifier, Style};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("colorscheme not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Default)]
pub struct Colorscheme {
    pub name: String,
    /// Group name → style. Group names match vim's `:highlight Group ...`.
    pub groups: HashMap<String, Style>,
    /// `hi link FROM TO` produces aliases — looked up lazily by [`style_for`].
    pub links: HashMap<String, String>,
}

impl Colorscheme {
    pub fn style_for(&self, group: &str) -> Option<Style> {
        let mut current = group;
        // Follow `hi link` chains. Cap depth so a malformed file can't loop.
        for _ in 0..16 {
            if let Some(style) = self.groups.get(current) {
                return Some(*style);
            }
            if let Some(target) = self.links.get(current) {
                current = target;
            } else {
                return None;
            }
        }
        None
    }
}

/// Public entry point: locate the named scheme on disk and parse it. The
/// returned scheme has vim's standard `syncolor.vim` defaults filled in
/// for any group the user's file didn't explicitly set.
pub fn load(name: &str) -> Result<Colorscheme, LoadError> {
    let path = resolve_path(name).ok_or_else(|| LoadError::NotFound(name.into()))?;
    let text = std::fs::read_to_string(&path)?;
    let mut scheme = parse(name, &text);
    apply_vim_defaults(&mut scheme);
    Ok(scheme)
}

/// Build a "no file found" scheme with just the vim defaults. Used as a
/// last-resort fallback so we always have *some* styling.
pub fn defaults() -> Colorscheme {
    let mut scheme = Colorscheme {
        name: "defaults".into(),
        ..Default::default()
    };
    apply_vim_defaults(&mut scheme);
    scheme
}

/// Fill in standard SynLink mappings and SynColor base-group styles from
/// vim's `runtime/syntax/syncolor.vim`. Anything the caller-supplied scheme
/// already defines is left alone.
pub fn apply_vim_defaults(scheme: &mut Colorscheme) {
    let link_defaults = [
        // SynLink chains from syncolor.vim — without these, `cKeyword` →
        // `Keyword` → ???; with these, `Keyword` → `Statement` (a real style).
        ("String", "Constant"),
        ("Character", "Constant"),
        ("Number", "Constant"),
        ("Boolean", "Constant"),
        ("Float", "Number"),
        ("Function", "Identifier"),
        ("Conditional", "Statement"),
        ("Repeat", "Statement"),
        ("Label", "Statement"),
        ("Operator", "Statement"),
        ("Keyword", "Statement"),
        ("Exception", "Statement"),
        ("Include", "PreProc"),
        ("Define", "PreProc"),
        ("Macro", "PreProc"),
        ("PreCondit", "PreProc"),
        ("StorageClass", "Type"),
        ("Structure", "Type"),
        ("Typedef", "Type"),
        ("Tag", "Special"),
        ("SpecialChar", "Special"),
        ("Delimiter", "Special"),
        ("SpecialComment", "Special"),
        ("Debug", "Special"),
    ];
    for (from, to) in link_defaults {
        scheme.links.entry(from.into()).or_insert_with(|| to.into());
    }
    // Base-group default colors (dark-bg variant from syncolor.vim).
    let color_defaults: &[(&str, Style)] = &[
        ("Comment", Style::default().fg(Color::Rgb(0x80, 0xa0, 0xff))),
        ("Constant", Style::default().fg(Color::Rgb(0xff, 0xa0, 0xa0))),
        ("Special", Style::default().fg(Color::Rgb(0xff, 0xa5, 0x00))),
        ("Identifier", Style::default().fg(Color::Rgb(0x40, 0xff, 0xff))),
        ("Statement", Style::default()
            .fg(Color::Rgb(0xff, 0xff, 0x60))
            .add_modifier(Modifier::BOLD)),
        ("PreProc", Style::default().fg(Color::Rgb(0xff, 0x80, 0xff))),
        ("Type", Style::default()
            .fg(Color::Rgb(0x60, 0xff, 0x60))
            .add_modifier(Modifier::BOLD)),
        ("Underlined", Style::default()
            .fg(Color::Rgb(0x80, 0xa0, 0xff))
            .add_modifier(Modifier::UNDERLINED)),
        ("Error", Style::default()
            .fg(Color::White)
            .bg(Color::Red)),
        ("Todo", Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)),
        ("Title", Style::default()
            .fg(Color::Rgb(0xff, 0x40, 0xff))
            .add_modifier(Modifier::BOLD)),
        ("Search", Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(0xc0, 0xc0, 0x00))),
    ];
    for (group, style) in color_defaults {
        scheme.groups.entry((*group).into()).or_insert(*style);
    }
}

/// First path that exists on disk, in priority order.
pub fn resolve_path(name: &str) -> Option<PathBuf> {
    let filename = format!("{name}.vim");
    for base in candidate_dirs() {
        let p = base.join(&filename);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.push(PathBuf::from("./colors"));
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        out.push(PathBuf::from(xdg).join("jvim/colors"));
    }
    if let Ok(home) = std::env::var("HOME") {
        out.push(PathBuf::from(home).join(".config/jvim/colors"));
    }
    // System vim install directory: /usr/share/vim/vim*/colors/.
    if let Ok(entries) = std::fs::read_dir("/usr/share/vim") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("vim") && s != "vimfiles" {
                out.push(entry.path().join("colors"));
            }
        }
    }
    out
}

// ---- Parser ----------------------------------------------------------------

pub fn parse(name: &str, text: &str) -> Colorscheme {
    let mut scheme = Colorscheme {
        name: name.into(),
        ..Default::default()
    };
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        // Vim allows `hi` / `highlight` (with optional `!`). Strip leading word.
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("highlight") || lower.starts_with("hi ") || lower == "hi" {
            let rest = strip_command(line);
            parse_highlight(rest, &mut scheme);
        }
        // Everything else (let, set, if, syntax, finish, etc.) is ignored.
    }
    scheme
}

fn strip_comment(line: &str) -> &str {
    // In a vim file, a leading `"` is a comment. Inline trailing `"` is
    // ambiguous (could be inside a string). Only strip leading-only comments
    // here to keep things conservative.
    let trimmed = line.trim_start();
    if trimmed.starts_with('"') {
        ""
    } else {
        line
    }
}

fn strip_command(line: &str) -> &str {
    let line = line.trim_start();
    for prefix in ["highlight!", "highlight", "hi!", "hi"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    line
}

fn parse_highlight(rest: &str, scheme: &mut Colorscheme) {
    let trimmed = rest.trim();
    // `hi clear`: wipe everything but the name field.
    if trimmed == "clear" || trimmed.starts_with("clear ") {
        scheme.groups.clear();
        scheme.links.clear();
        return;
    }
    // `hi [def] link FROM TO` — alias.
    let after_def = trimmed
        .strip_prefix("def ")
        .or_else(|| trimmed.strip_prefix("default "))
        .unwrap_or(trimmed);
    if let Some(rest) = after_def.strip_prefix("link ") {
        let mut it = rest.split_whitespace();
        if let (Some(from), Some(to)) = (it.next(), it.next()) {
            scheme.links.insert(from.to_string(), to.to_string());
        }
        return;
    }
    // `hi Group key=val key=val ...`
    let mut tokens = after_def.split_whitespace();
    let Some(group) = tokens.next() else { return };
    let mut cterm_fg: Option<Color> = None;
    let mut cterm_bg: Option<Color> = None;
    let mut gui_fg: Option<Color> = None;
    let mut gui_bg: Option<Color> = None;
    let mut modifier = Modifier::empty();
    for tok in tokens {
        let Some(eq) = tok.find('=') else { continue };
        let key = tok[..eq].to_ascii_lowercase();
        let val = &tok[eq + 1..];
        if val.eq_ignore_ascii_case("none") || val.eq_ignore_ascii_case("bg") || val.is_empty() {
            continue;
        }
        match key.as_str() {
            "ctermfg" => cterm_fg = parse_cterm_color(val),
            "ctermbg" => cterm_bg = parse_cterm_color(val),
            "guifg" => gui_fg = parse_gui_color(val),
            "guibg" => gui_bg = parse_gui_color(val),
            "cterm" | "gui" | "term" => modifier |= parse_attrs(val),
            _ => {}
        }
    }
    let prefer_truecolor = terminal_supports_truecolor();
    let fg = if prefer_truecolor {
        gui_fg.or(cterm_fg)
    } else {
        cterm_fg.or(gui_fg)
    };
    let bg = if prefer_truecolor {
        gui_bg.or(cterm_bg)
    } else {
        cterm_bg.or(gui_bg)
    };
    let mut style = Style::default();
    if let Some(c) = fg {
        style = style.fg(c);
    }
    if let Some(c) = bg {
        style = style.bg(c);
    }
    if !modifier.is_empty() {
        style = style.add_modifier(modifier);
    }
    scheme.groups.insert(group.to_string(), style);
}

/// Trust `COLORTERM=truecolor` / `=24bit` to indicate 24-bit-capable
/// terminals; otherwise fall back to 16-color cterm escapes which work
/// almost everywhere.
fn terminal_supports_truecolor() -> bool {
    matches!(
        std::env::var("COLORTERM").as_deref(),
        Ok("truecolor") | Ok("24bit")
    )
}

fn parse_cterm_color(s: &str) -> Option<Color> {
    let lower = s.to_ascii_lowercase();
    Some(match lower.as_str() {
        "black" => Color::Black,
        "darkred" => Color::Red,
        "darkgreen" => Color::Green,
        "darkyellow" | "brown" => Color::Yellow,
        "darkblue" => Color::Blue,
        "darkmagenta" => Color::Magenta,
        "darkcyan" => Color::Cyan,
        "lightgray" | "lightgrey" | "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "red" | "lightred" => Color::LightRed,
        "green" | "lightgreen" => Color::LightGreen,
        "yellow" | "lightyellow" => Color::LightYellow,
        "blue" | "lightblue" => Color::LightBlue,
        "magenta" | "lightmagenta" => Color::LightMagenta,
        "cyan" | "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return s.parse::<u8>().ok().map(Color::Indexed),
    })
}

fn parse_gui_color(s: &str) -> Option<Color> {
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
    }
    parse_cterm_color(s)
}

fn parse_attrs(val: &str) -> Modifier {
    let mut m = Modifier::empty();
    for attr in val.split(',') {
        match attr.trim().to_ascii_lowercase().as_str() {
            "bold" => m |= Modifier::BOLD,
            "italic" => m |= Modifier::ITALIC,
            "underline" => m |= Modifier::UNDERLINED,
            "reverse" | "inverse" => m |= Modifier::REVERSED,
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_highlight_prefers_cterm_when_no_truecolor() {
        // Force the non-truecolor branch by clearing COLORTERM for this run.
        let prev = std::env::var("COLORTERM").ok();
        std::env::remove_var("COLORTERM");
        let scheme = parse(
            "test",
            r#"
            " This is a comment
            highlight Comment ctermfg=cyan guifg=#80a0ff term=bold cterm=bold
            highlight Search ctermbg=3 guibg=#c0c000
            "#,
        );
        let c = scheme.style_for("Comment").unwrap();
        // Without truecolor support, cterm wins so the styling renders on
        // a basic terminal.
        assert!(matches!(c.fg, Some(Color::LightCyan) | Some(Color::Cyan)));
        assert!(c.add_modifier.contains(Modifier::BOLD));
        let s = scheme.style_for("Search").unwrap();
        assert!(matches!(s.bg, Some(Color::Indexed(3)) | Some(Color::Yellow)));
        if let Some(v) = prev {
            std::env::set_var("COLORTERM", v);
        }
    }

    #[test]
    fn truecolor_terminal_prefers_gui_rgb_when_both_given() {
        let prev = std::env::var("COLORTERM").ok();
        std::env::set_var("COLORTERM", "truecolor");
        let scheme = parse(
            "test",
            "highlight Comment ctermfg=cyan guifg=#80a0ff cterm=bold",
        );
        let c = scheme.style_for("Comment").unwrap();
        assert_eq!(c.fg, Some(Color::Rgb(0x80, 0xa0, 0xff)));
        assert!(c.add_modifier.contains(Modifier::BOLD));
        match prev {
            Some(v) => std::env::set_var("COLORTERM", v),
            None => std::env::remove_var("COLORTERM"),
        }
    }

    #[test]
    fn hi_link_aliases_group() {
        let scheme = parse(
            "x",
            r#"
            highlight Statement ctermfg=yellow
            hi link rustKeyword Statement
            "#,
        );
        let s = scheme.style_for("rustKeyword").unwrap();
        // In vim's cterm palette `yellow` is bright (index 11), mapped to
        // ratatui's LightYellow. Plain `darkyellow` / `brown` would give
        // regular Yellow.
        assert_eq!(s.fg, Some(Color::LightYellow));
    }

    #[test]
    fn hi_clear_wipes_state() {
        let scheme = parse(
            "x",
            r#"
            highlight Comment ctermfg=blue
            hi clear
            highlight Search ctermbg=red
            "#,
        );
        assert!(scheme.style_for("Comment").is_none());
        assert!(scheme.style_for("Search").is_some());
    }

    #[test]
    fn handles_def_link() {
        let scheme = parse(
            "x",
            r#"
            hi def link rustKeyword Keyword
            highlight Keyword ctermfg=red
            "#,
        );
        let s = scheme.style_for("rustKeyword").unwrap();
        assert_eq!(s.fg, Some(Color::LightRed));
    }

    #[test]
    fn ignores_unknown_keys_silently() {
        let scheme = parse(
            "x",
            "highlight Foo ctermfg=red unknownattr=whatever",
        );
        assert!(scheme.style_for("Foo").is_some());
    }

    #[test]
    fn loads_myfault2_from_local_dir() {
        // The fixture lives at ./colors/myfault2.vim relative to the
        // workspace root (project file). cargo test runs from there.
        let scheme = load("myfault2").expect("scheme should be findable");
        assert_eq!(scheme.name, "myfault2");
        assert!(scheme.style_for("Comment").is_some());
        assert!(scheme.style_for("Search").is_some());
    }
}
