//! Render a single window's buffer slice, plus selection highlight.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::cursor::Selection;
use crate::text::width as twidth;
use crate::window::Window;
use crate::Editor;

pub fn render(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let Some(buffer) = editor.buffers.get(&window.buffer) else {
        return;
    };
    let tab_width = editor.config.options.tab_width;

    let height = area.height as usize;
    let mut lines = Vec::with_capacity(height);
    let show_number = editor.config.options.number;
    // Diagnostic gutter: one column reserved at the very left if any LSP
    // client has diagnostics for this buffer's URI.
    let diag_lines = diagnostic_line_severities(editor, buffer);
    let diag_gutter = if diag_lines.is_empty() { 0 } else { 1 };
    let num_gutter = if show_number {
        let max_line = window.top_line + height;
        let digits = num_digits(max_line.max(1));
        digits + 1
    } else {
        0
    };
    let gutter = diag_gutter + num_gutter;
    let text_width = area.width.saturating_sub(gutter as u16) as usize;

    let sel_style = Style::default()
        .bg(Color::Rgb(60, 80, 110))
        .add_modifier(Modifier::REVERSED);

    // Fetch the cached syntax engine for this buffer. First access for a
    // given buffer pays the cost of reading the system .vim syntax file +
    // compiling regexes; every render after that is essentially free.
    let syntax = editor.syntax_for(window.buffer);
    let syntax = syntax.as_ref();

    // Incremental search: while the search prompt is open use the live
    // prompt regex; otherwise use the last committed pattern.
    let search_pat = if editor.mode == crate::mode::ModeId::Search {
        editor.search.prompt_re.as_ref()
    } else {
        editor.search.pattern.as_ref()
    };

    // Pre-index enough mmap lines for this viewport so the line_count() check
    // inside the loop is always accurate for every row we're about to render.
    buffer.ensure_lines_visible(window.top_line, height + 1);

    // Virtual text: collect per-line diagnostic messages if the option is on.
    let vtext = if editor.config.options.diagnostic_virtual_text {
        diagnostic_virtual_text_lines(editor, buffer)
    } else {
        std::collections::HashMap::new()
    };

    // Compute multiline string state at the start of top_line by scanning
    // all preceding lines. Only has an effect for Python (triple-quoted strings).
    let mut ml_state = {
        let mut s = crate::syntax::MultilineState::None;
        for idx in 0..window.top_line {
            if idx >= buffer.line_count() {
                break;
            }
            s = syntax.advance_state(&buffer.line_string(idx), s);
        }
        s
    };

    for row in 0..height {
        let line_idx = window.top_line + row;
        if line_idx >= buffer.line_count() {
            lines.push(Line::from(Span::styled(
                "~",
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }
        let line_text = buffer.line_string(line_idx);
        let mut spans = Vec::new();
        if diag_gutter > 0 {
            let (sym, color) = match diag_lines.get(&(line_idx as u32)) {
                Some(s) => match s {
                    DiagSev::Error => ("!", Color::Red),
                    DiagSev::Warning => ("?", Color::Yellow),
                    DiagSev::Info => ("i", Color::Blue),
                    DiagSev::Hint => ("h", Color::Cyan),
                },
                None => (" ", Color::Reset),
            };
            spans.push(Span::styled(sym, Style::default().fg(color)));
        }
        if show_number {
            let n = format!(
                "{:>width$} ",
                line_idx + 1,
                width = num_gutter.saturating_sub(1).max(1)
            );
            spans.push(Span::styled(n, Style::default().fg(Color::DarkGray)));
        }
        let (sel_start_col, sel_end_col) =
            selection_cols_for_row(window, buffer, line_idx, tab_width);

        // Per-byte syntax group lookup table for this line.
        let cur_ml_state = ml_state;
        ml_state = syntax.advance_state(&line_text, cur_ml_state);
        let syntax_groups = build_syntax_groups(&line_text, syntax, search_pat, cur_ml_state);
        // Persistent text highlights (`:highlight foo` / `<leader>m`).
        let highlight_overlay = build_highlight_overlay(&line_text, &editor.highlights);
        // Optional red-background overlay for trailing whitespace + tabs.
        let ws_overlay = build_whitespace_overlay(
            &line_text,
            editor.config.options.highlight_trailing_whitespace,
            editor.config.options.highlight_tabs,
        );
        spans.extend(line_spans(
            &line_text,
            window.left_col,
            text_width,
            tab_width,
            sel_start_col,
            sel_end_col,
            sel_style,
            &syntax_groups,
            &highlight_overlay,
            &ws_overlay,
            &editor.colorscheme,
        ));
        // Append virtual-text diagnostic if enabled and there's a message.
        if let Some((sev, msg)) = vtext.get(&(line_idx as u32)) {
            let marker_color = match sev {
                DiagSev::Error   => Color::Red,
                DiagSev::Warning => Color::Yellow,
                DiagSev::Info    => Color::Blue,
                DiagSev::Hint    => Color::Cyan,
            };
            spans.push(Span::styled(
                "  \u{25a0} ",
                Style::default().fg(marker_color),
            ));
            spans.push(Span::styled(
                msg.clone(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);

    // Color column(s): paint a vertical background ruler (vim's
    // `colorcolumn`) over the real text rows, after the paragraph so it shows
    // through empty cells past end-of-line too. Painted directly on the frame
    // buffer rather than via spans so the bar reaches the full line height.
    let cols = color_columns(&editor.config.options.color_column, editor.config.options.textwidth);
    if !cols.is_empty() {
        let cc_bg = Color::Rgb(64, 48, 48);
        let text_x0 = area.x + gutter as u16;
        let text_x_end = area.x + area.width;
        let real_rows = buffer
            .line_count()
            .saturating_sub(window.top_line)
            .min(height) as u16;
        let buf = frame.buffer_mut();
        for &dc in &cols {
            if dc < window.left_col {
                continue; // scrolled off to the left
            }
            let x = text_x0 + (dc - window.left_col) as u16;
            if x >= text_x_end {
                continue; // off the right edge
            }
            for r in 0..real_rows {
                if let Some(cell) = buf.cell_mut((x, area.y + r)) {
                    cell.set_bg(cc_bg);
                }
            }
        }
    }
}

/// Parse `options.color_column` into 0-based display columns to highlight.
/// Entries are 1-based screen columns; a leading `+`/`-` makes them relative
/// to `textwidth` (e.g. `+1` = the column just past it). Blank or invalid
/// entries are skipped.
fn color_columns(spec: &str, textwidth: usize) -> Vec<usize> {
    let mut cols = Vec::new();
    for item in spec.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let one_based = if let Some(n) = item.strip_prefix('+') {
            n.parse::<usize>().ok().map(|n| textwidth.saturating_add(n))
        } else if let Some(n) = item.strip_prefix('-') {
            n.parse::<usize>().ok().and_then(|n| textwidth.checked_sub(n))
        } else {
            item.parse::<usize>().ok()
        };
        // 1-based screen column → 0-based display index; column 0 is invalid.
        if let Some(c) = one_based.filter(|&c| c >= 1) {
            cols.push(c - 1);
        }
    }
    cols
}

/// Per-byte Style overlay for trailing whitespace + tab markers. Cells
/// that satisfy either condition (driven by `options.highlight_*`) get
/// a red background; everything else gets `None` so the layer below
/// shows through.
fn build_whitespace_overlay(
    line: &str,
    mark_trailing: bool,
    mark_tabs: bool,
) -> Vec<Option<ratatui::style::Style>> {
    let mut buf: Vec<Option<ratatui::style::Style>> = vec![None; line.len()];
    if !mark_trailing && !mark_tabs {
        return buf;
    }
    let red = ratatui::style::Style::default().bg(ratatui::style::Color::Red);
    // Byte index of the first cell of the trailing-whitespace run, if any.
    // `line.trim_end_matches(...)` returns the slice WITHOUT trailing
    // whitespace; its length tells us where the trail begins.
    let trail_from = if mark_trailing {
        line.trim_end_matches(|c: char| c == ' ' || c == '\t' || c == '\r')
            .len()
    } else {
        usize::MAX
    };
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        let is_tab = b == b'\t';
        let is_trailing = mark_trailing && i >= trail_from && (b == b' ' || b == b'\t');
        if (mark_tabs && is_tab) || is_trailing {
            buf[i] = Some(red);
        }
    }
    buf
}

/// Per-byte Style overlay from the editor's text highlights. Painted
/// over the syntax layer but under the selection.
fn build_highlight_overlay(
    line: &str,
    hls: &crate::highlights::Highlights,
) -> Vec<Option<ratatui::style::Style>> {
    let mut buf: Vec<Option<ratatui::style::Style>> = vec![None; line.len()];
    if hls.is_empty() {
        return buf;
    }
    for entry in &hls.entries {
        for m in entry.regex.find_iter(line) {
            let style = entry.style();
            for i in m.start()..m.end() {
                if i < buf.len() {
                    buf[i] = Some(style);
                }
            }
        }
    }
    buf
}

/// Build a `Vec<Option<&str>>` mapping each byte in `line` to its highlight
/// group name (if any). Search matches override syntax (Search wins).
fn build_syntax_groups<'a>(
    line: &str,
    syntax: &'a crate::syntax::Syntax,
    search_pat: Option<&regex::Regex>,
    ml_state: crate::syntax::MultilineState,
) -> Vec<Option<String>> {
    let mut buf: Vec<Option<String>> = vec![None; line.len()];
    for (range, group) in syntax.highlight_line_ctx(line, ml_state).0 {
        for i in range.clone() {
            if i < buf.len() {
                buf[i] = Some(group.clone());
            }
        }
    }
    if let Some(re) = search_pat {
        for m in re.find_iter(line) {
            for i in m.start()..m.end() {
                if i < buf.len() {
                    buf[i] = Some("Search".into());
                }
            }
        }
    }
    buf
}

/// Return `(start_col, end_col)` of the selection within `line_idx`, in
/// display columns. `end_col` is *exclusive*. `None` means no selection on
/// this row, encoded as `(0, 0)` => no highlight.
fn selection_cols_for_row(
    window: &Window,
    buffer: &crate::buffer::Buffer,
    line_idx: usize,
    tab_width: usize,
) -> (usize, usize) {
    let line = buffer.line_string(line_idx);
    let line_width = twidth::line_display_width(&line, tab_width);
    match window.selection {
        Selection::None => (0, 0),
        Selection::Char { anchor } => {
            let cur = window.cursor;
            let (lo, hi) = if (anchor.row, anchor.col) <= (cur.row, cur.col) {
                (anchor, cur)
            } else {
                (cur, anchor)
            };
            if line_idx < lo.row || line_idx > hi.row {
                (0, 0)
            } else if lo.row == hi.row {
                (lo.col, hi.col + 1)
            } else if line_idx == lo.row {
                (lo.col, line_width + 1)
            } else if line_idx == hi.row {
                (0, hi.col + 1)
            } else {
                (0, line_width + 1)
            }
        }
        Selection::Line { anchor_row } => {
            let (lo, hi) = if anchor_row <= window.cursor.row {
                (anchor_row, window.cursor.row)
            } else {
                (window.cursor.row, anchor_row)
            };
            if line_idx < lo || line_idx > hi {
                (0, 0)
            } else {
                (0, line_width.max(1))
            }
        }
        Selection::Block { anchor } => {
            let cur = window.cursor;
            let (top, bot) = (anchor.row.min(cur.row), anchor.row.max(cur.row));
            let (left, right) = (anchor.col.min(cur.col), anchor.col.max(cur.col));
            if line_idx < top || line_idx > bot {
                (0, 0)
            } else {
                (left, right + 1)
            }
        }
    }
}

pub fn set_cursor(editor: &Editor, window: &Window, frame: &mut Frame, area: Rect) {
    let diag_gutter = if editor
        .buffers
        .get(&window.buffer)
        .and_then(|b| b.path())
        .map(|p| {
            lsp_types::Url::from_file_path(p)
                .ok()
                .map(|u| u.to_string())
                .map(|uri| {
                    editor
                        .lsp
                        .clients
                        .values()
                        .any(|c| !c.diagnostics.for_uri(&uri).is_empty())
                })
                .unwrap_or(false)
        })
        .unwrap_or(false)
    {
        1
    } else {
        0
    };
    let num_gutter = if editor.config.options.number {
        let max_line = window.top_line + area.height as usize;
        num_digits(max_line.max(1)) + 1
    } else {
        0
    };
    let gutter = diag_gutter + num_gutter;
    let screen_row = window.cursor.row.saturating_sub(window.top_line);
    let screen_col = window.cursor.col.saturating_sub(window.left_col);
    if screen_row >= area.height as usize {
        return;
    }
    let x = area.x + gutter as u16 + screen_col as u16;
    let y = area.y + screen_row as u16;
    frame.set_cursor_position((x, y));
}

/// Build one or more `Span`s for a single text line, splitting whenever
/// the resolved style changes. Priority (highest first):
/// selection > trailing-ws / tab markers > text-highlight overlay > syntax.
#[allow(clippy::too_many_arguments)]
fn line_spans(
    line: &str,
    left_col: usize,
    width: usize,
    tab_width: usize,
    sel_start_col: usize,
    sel_end_col: usize,
    sel_style: Style,
    syntax_groups: &[Option<String>],
    highlight_overlay: &[Option<Style>],
    ws_overlay: &[Option<Style>],
    scheme: &crate::colorscheme::Colorscheme,
) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }
    let mut col = 0usize;
    let mut emitted = 0usize;
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let mut spans: Vec<Span<'static>> = Vec::new();

    let flush = |spans: &mut Vec<Span<'static>>, text: &mut String, style: Style| {
        if !text.is_empty() {
            spans.push(Span::styled(std::mem::take(text), style));
        }
    };

    for (byte_offset, g, _gc, w) in twidth::graphemes_with_cols(line, tab_width) {
        if col + w <= left_col {
            col += w;
            continue;
        }
        if emitted >= width {
            break;
        }
        let selected = col >= sel_start_col && col < sel_end_col;
        let group = syntax_groups.get(byte_offset).and_then(|g| g.as_deref());
        let style = if selected {
            sel_style
        } else if let Some(ws) = ws_overlay.get(byte_offset).copied().flatten() {
            ws
        } else if let Some(hl) = highlight_overlay.get(byte_offset).copied().flatten() {
            hl
        } else {
            group
                .and_then(|name| scheme.style_for(name))
                .unwrap_or_default()
        };
        if style != current_style {
            flush(&mut spans, &mut current_text, current_style);
            current_style = style;
        }
        if g == "\t" {
            let start_skip = left_col.saturating_sub(col);
            let visible = w.saturating_sub(start_skip);
            let take = visible.min(width - emitted);
            for _ in 0..take {
                current_text.push(' ');
            }
            emitted += take;
        } else {
            if emitted + w > width {
                break;
            }
            current_text.push_str(g);
            emitted += w;
        }
        col += w;
    }
    flush(&mut spans, &mut current_text, current_style);
    spans
}

fn num_digits(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        (n as f64).log10().floor() as usize + 1
    }
}

#[derive(Copy, Clone, Debug)]
enum DiagSev {
    Error,
    Warning,
    Info,
    Hint,
}

/// Walk every LSP client and collect the highest-severity diagnostic message
/// on each line of this buffer for virtual-text display.
/// Returns `line → (severity, first_line_of_message)`.
fn diagnostic_virtual_text_lines(
    editor: &Editor,
    buffer: &crate::buffer::Buffer,
) -> std::collections::HashMap<u32, (DiagSev, String)> {
    let mut out: std::collections::HashMap<u32, (DiagSev, String)> =
        std::collections::HashMap::new();
    let Some(path) = buffer.path() else {
        return out;
    };
    let Ok(uri) = lsp_types::Url::from_file_path(path) else {
        return out;
    };
    let uri_str = uri.to_string();
    let rank = |s: DiagSev| match s {
        DiagSev::Error => 3,
        DiagSev::Warning => 2,
        DiagSev::Info => 1,
        DiagSev::Hint => 0,
    };
    // Clamp to the last rendered line. Servers (e.g. clangd) often report
    // end-of-file errors one past the trailing newline, which buffer::line_count()
    // drops — clamping ensures those diagnostics are visible on the last line.
    let last_line = buffer.line_count().saturating_sub(1) as u32;
    for client in editor.lsp.clients.values() {
        for d in client.diagnostics.for_uri(&uri_str) {
            let sev = match d.severity {
                Some(lsp_types::DiagnosticSeverity::ERROR) => DiagSev::Error,
                Some(lsp_types::DiagnosticSeverity::WARNING) => DiagSev::Warning,
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => DiagSev::Info,
                Some(lsp_types::DiagnosticSeverity::HINT) => DiagSev::Hint,
                _ => DiagSev::Info,
            };
            let line = d.range.start.line.min(last_line);
            let msg = d.message.lines().next().unwrap_or("").to_string();
            let entry = out.entry(line).or_insert((sev, msg.clone()));
            if rank(sev) > rank(entry.0) {
                *entry = (sev, msg);
            }
        }
    }
    out
}

/// Walk every LSP client and collect the most-severe diagnostic on each
/// line of this buffer. Lines without diagnostics are omitted.
fn diagnostic_line_severities(
    editor: &Editor,
    buffer: &crate::buffer::Buffer,
) -> std::collections::HashMap<u32, DiagSev> {
    let mut out = std::collections::HashMap::new();
    let Some(path) = buffer.path() else {
        return out;
    };
    let Ok(uri) = lsp_types::Url::from_file_path(path) else {
        return out;
    };
    let uri_str = uri.to_string();
    // Same clamp as virtual text: keep end-of-file diagnostics on last line.
    let last_line = buffer.line_count().saturating_sub(1) as u32;
    for client in editor.lsp.clients.values() {
        for d in client.diagnostics.for_uri(&uri_str) {
            let new_sev = match d.severity {
                Some(lsp_types::DiagnosticSeverity::ERROR) => DiagSev::Error,
                Some(lsp_types::DiagnosticSeverity::WARNING) => DiagSev::Warning,
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => DiagSev::Info,
                Some(lsp_types::DiagnosticSeverity::HINT) => DiagSev::Hint,
                _ => DiagSev::Info,
            };
            let line = d.range.start.line.min(last_line);
            let cur_rank = |s: DiagSev| match s {
                DiagSev::Error => 3,
                DiagSev::Warning => 2,
                DiagSev::Info => 1,
                DiagSev::Hint => 0,
            };
            let entry = out.entry(line).or_insert(new_sev);
            if cur_rank(new_sev) > cur_rank(*entry) {
                *entry = new_sev;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::color_columns;

    #[test]
    fn empty_spec_is_off() {
        assert!(color_columns("", 80).is_empty());
        assert!(color_columns("   ", 80).is_empty());
    }

    #[test]
    fn absolute_columns_are_zero_based() {
        assert_eq!(color_columns("80", 0), vec![79]);
        assert_eq!(color_columns("1", 0), vec![0]);
        // Column 0 is invalid and dropped.
        assert_eq!(color_columns("0", 0), Vec::<usize>::new());
    }

    #[test]
    fn relative_to_textwidth() {
        assert_eq!(color_columns("+1", 80), vec![80]); // column 81 → idx 80
        assert_eq!(color_columns("-1", 80), vec![78]); // column 79 → idx 78
        // Underflow is skipped rather than wrapping.
        assert_eq!(color_columns("-5", 2), Vec::<usize>::new());
    }

    #[test]
    fn list_and_bad_entries() {
        assert_eq!(color_columns("3,7,bad,", 80), vec![2, 6]);
    }
}
