use crate::Editor;

/// One styled segment of a render-buffer line, as produced by a plugin via the
/// render ABI. Colours are RGB; `size` is an advisory scale (0 = normal, 1..=6
/// = heading levels) that the TUI approximates with emphasis; `link` makes the
/// segment a followable hyperlink to that target.
#[derive(Debug, Clone, Default)]
pub struct RenderSpanSpec {
    pub text: String,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub strike: bool,
    pub size: u8,
    pub link: Option<String>,
}

/// One render-buffer line: a sequence of styled segments.
pub type RenderLineSpec = Vec<RenderSpanSpec>;

/// A mutation the plugin requested during a WASM call.
/// Collected in `HostData::pending`, replayed onto `&mut Editor` after the
/// call returns — no borrow conflict because the WASM frame has already exited.
#[derive(Debug)]
pub enum PendingAction {
    SetStatus(String),
    Log(String),
    InsertText { buffer_id: u32, char_pos: usize, text: String },
    DeleteText { buffer_id: u32, start: usize, end: usize },
    /// Replace lines [start, end) with `lines` (Neovim nvim_buf_set_lines semantics).
    SetLines { buffer_id: u32, start: i64, end: i64, lines: Vec<String> },
    SetCursor { window_id: u32, row: usize, col: usize },
    /// Set an editor option by name (tab_width / expandtab / number / …).
    SetOption { name: String, value: String },
    /// Plugin asked to register an ex command. Returned from apply_pending so
    /// the caller can create the PluginCommand in CommandRegistry.
    RegisterCommand { name: String },
    /// Plugin asked to bind a key to one of its own functions.
    BindKey { mode: String, keys: String, plugin_name: String, function: String },
    /// Plugin declared itself as the loader for plugins with this file extension.
    RegisterPluginManager { ext: String },
    /// Plugin declared itself as the indent provider for a filetype.
    RegisterIndentProvider { filetype: String },
    /// Open a non-editable "render buffer" of pre-styled lines in a split.
    /// `producer` is the command that built it (re-run to follow links). Built
    /// via the render ABI (`rtdvi_render_span`/`_newline`/`_open`); Lua/WASM
    /// agnostic at the apply layer.
    OpenRenderBuffer { title: String, producer: Option<String>, lines: Vec<RenderLineSpec> },
}

/// Convert render specs into the styled `Line`s the renderer paints, the plain
/// text that backs the rope (so motions/scroll work), and the link regions
/// (in display columns) the follow action uses. Pure — unit-tested.
pub fn build_render_lines(
    specs: &[RenderLineSpec],
    tab_width: usize,
) -> (String, Vec<ratatui::text::Line<'static>>, Vec<crate::buffer::RenderLink>) {
    use crate::buffer::RenderLink;
    use crate::text::width as twidth;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    let mut plain = String::new();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(specs.len());
    let mut links: Vec<RenderLink> = Vec::new();

    for (row, spec_line) in specs.iter().enumerate() {
        if row > 0 {
            plain.push('\n');
        }
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(spec_line.len());
        let mut col = 0usize; // display column within this line
        for sp in spec_line {
            let mut style = Style::default();
            if let Some((r, g, b)) = sp.fg {
                style = style.fg(Color::Rgb(r, g, b));
            }
            if let Some((r, g, b)) = sp.bg {
                style = style.bg(Color::Rgb(r, g, b));
            }
            if sp.bold || sp.size >= 1 {
                style = style.add_modifier(Modifier::BOLD);
            }
            if sp.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if sp.underline || sp.link.is_some() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if sp.reverse {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if sp.strike {
                style = style.add_modifier(Modifier::CROSSED_OUT);
            }
            let w = twidth::line_display_width(&sp.text, tab_width);
            if let Some(target) = &sp.link {
                links.push(RenderLink {
                    line: row,
                    start_col: col,
                    end_col: col + w,
                    target: target.clone(),
                });
            }
            col += w;
            plain.push_str(&sp.text);
            spans.push(Span::styled(sp.text.clone(), style));
        }
        lines.push(Line::from(spans));
    }
    (plain, lines, links)
}

/// Create a render buffer from `lines` and show it. On the first call this
/// splits the active window; a subsequent call (e.g. following a link) reuses
/// the tab's existing render window so the new page replaces it in place —
/// browser-style navigation.
fn open_render_buffer(
    editor: &mut Editor,
    title: String,
    producer: Option<String>,
    lines: Vec<RenderLineSpec>,
) {
    use crate::buffer::RenderContent;
    use crate::window::SplitAxis;

    let tab_width = editor.config.options.tab_width;
    let (plain, styled, links) = build_render_lines(&lines, tab_width);

    // The window the producer read from — used to re-render followed pages.
    let source_window = editor.tabs.get(editor.active_tab).map(|t| t.active);
    let base_dir = source_window
        .and_then(|wid| editor.windows.get(&wid))
        .map(|w| w.buffer)
        .and_then(|bid| editor.buffers.get(&bid))
        .and_then(|b| b.path())
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let content = RenderContent { lines: styled, links, producer, base_dir, source_window };
    let id = editor.new_buffer_id();
    editor
        .buffers
        .insert(id, crate::buffer::Buffer::render(id, title, &plain, content));
    crate::event::emit(editor, crate::event::Event::BufferOpened(id));

    // Reuse an existing render window in this tab (replace its page), else split.
    let existing = editor.tabs.get(editor.active_tab).and_then(|tab| {
        tab.tree.windows().into_iter().find(|wid| {
            editor
                .windows
                .get(wid)
                .and_then(|w| editor.buffers.get(&w.buffer))
                .map(|b| !b.is_editable())
                .unwrap_or(false)
        })
    });

    let target_win = match existing {
        Some(wid) => {
            let old_buf = editor.windows.get(&wid).map(|w| w.buffer);
            if let Some(tab) = editor.tabs.get_mut(editor.active_tab) {
                tab.active = wid;
            }
            if let Some(old) = old_buf {
                editor.buffers.remove(&old);
            }
            Some(wid)
        }
        None => {
            crate::window_actions::split_active(editor, SplitAxis::Horizontal);
            editor.tabs.get(editor.active_tab).map(|t| t.active)
        }
    };

    if let Some(target_win) = target_win {
        if let Some(w) = editor.windows.get_mut(&target_win) {
            w.buffer = id;
            w.cursor = crate::cursor::Cursor::default();
            w.top_line = 0;
            w.left_col = 0;
            w.selection = crate::cursor::Selection::None;
        }
    }
    crate::mode::sync_mode_for_active(editor);
}

/// Result of replaying pending actions.
pub struct ApplyResult {
    /// Command names registered via `rtdvi_register_command`.
    pub new_commands: Vec<String>,
    /// File extensions this plugin declared itself manager for (e.g. `".lua"`).
    pub new_manager_exts: Vec<String>,
    /// Filetypes this plugin declared itself indent provider for (e.g. `"c"`).
    pub new_indent_filetypes: Vec<String>,
    /// Lines logged via `rtdvi_log` during the call, in order.
    pub log_lines: Vec<String>,
}

/// Replay a batch of pending actions onto the editor.
pub fn apply_pending(
    editor: &mut Editor,
    actions: Vec<PendingAction>,
    plugin_name: &str,
) -> ApplyResult {
    use crate::buffer::BufferId;
    use crate::window::WindowId;

    let mut result = ApplyResult { new_commands: Vec::new(), new_manager_exts: Vec::new(), new_indent_filetypes: Vec::new(), log_lines: Vec::new() };

    for action in actions {
        match action {
            PendingAction::Log(msg) => {
                tracing::info!("[plugin:{plugin_name}] {msg}");
                result.log_lines.push(msg);
            }
            PendingAction::SetStatus(msg) => {
                result.log_lines.push(format!("[status] {msg}"));
                editor.status_message = Some(msg);
            }
            PendingAction::InsertText { buffer_id, char_pos, text } => {
                if let Some(buf) = editor.buffers.get_mut(&BufferId(buffer_id)) {
                    buf.insert(char_pos, &text);
                }
            }
            PendingAction::DeleteText { buffer_id, start, end } => {
                if let Some(buf) = editor.buffers.get_mut(&BufferId(buffer_id)) {
                    buf.delete(start..end);
                }
            }
            PendingAction::SetLines { buffer_id, start, end, lines } => {
                if let Some(buf) = editor.buffers.get_mut(&BufferId(buffer_id)) {
                    let lc = buf.line_count() as i64;
                    let s = if start < 0 { (lc + start).max(0) } else { start.min(lc) } as usize;
                    let e = if end < 0 { (lc + end + 1).max(0) } else { end.min(lc) } as usize;
                    let start_char = buf.line_to_char(s);
                    let end_char = buf.line_to_char(e.min(buf.line_count()));
                    let mut text = lines.join("\n");
                    if !lines.is_empty() { text.push('\n'); }
                    buf.replace(start_char..end_char, &text);
                }
            }
            PendingAction::SetCursor { window_id, row, col } => {
                if let Some(win) = editor.windows.get_mut(&WindowId(window_id)) {
                    win.cursor.row = row;
                    win.cursor.col = col;
                }
            }
            PendingAction::SetOption { name, value } => {
                let opts = &mut editor.config.options;
                match name.as_str() {
                    "tab_width" | "tabstop" | "ts" | "shiftwidth" | "sw" => {
                        if let Ok(n) = value.parse::<usize>() { opts.tab_width = n; }
                    }
                    "expandtab" | "et" => {
                        opts.expandtab = matches!(value.as_str(), "true" | "1");
                    }
                    "number" | "nu" => {
                        opts.number = matches!(value.as_str(), "true" | "1");
                    }
                    _ => tracing::debug!("set_option: unknown option {name:?}"),
                }
            }
            PendingAction::OpenRenderBuffer { title, producer, lines } => {
                open_render_buffer(editor, title, producer, lines);
            }
            PendingAction::RegisterCommand { name } => {
                result.new_commands.push(name);
            }
            PendingAction::RegisterPluginManager { ext } => {
                result.new_manager_exts.push(ext);
            }
            PendingAction::RegisterIndentProvider { filetype } => {
                result.new_indent_filetypes.push(filetype);
            }
            PendingAction::BindKey { mode, keys, plugin_name: pname, function } => {
                use crate::keymap::Action;
                use crate::mode::ModeId;
                let mode_id = match mode.as_str() {
                    "normal" | "n" => ModeId::Normal,
                    "insert" | "i" => ModeId::Insert,
                    "visual" | "v" => ModeId::Visual,
                    "vline" | "V" => ModeId::VisualLine,
                    "vblock" => ModeId::VisualBlock,
                    _ => {
                        tracing::warn!(
                            "[plugin:{plugin_name}] unknown mode for bind_key: {mode}"
                        );
                        continue;
                    }
                };
                let ex = format!("plugin {pname}.{function}()");
                let expanded = keys.replace(
                    "<leader>",
                    &editor.config.options.leader.to_string(),
                );
                if let Err(e) = editor.keymap.bind(mode_id, &expanded, Action::Ex(ex)) {
                    tracing::warn!("[plugin:{plugin_name}] bind_key failed: {e}");
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn span(text: &str) -> RenderSpanSpec {
        RenderSpanSpec { text: text.into(), ..Default::default() }
    }

    #[test]
    fn build_render_lines_plain_text_links_and_styles() {
        let specs = vec![
            vec![
                span("see "),
                RenderSpanSpec { text: "docs".into(), link: Some("guide.md".into()), ..Default::default() },
            ],
            vec![RenderSpanSpec { text: "X".into(), fg: Some((255, 0, 0)), bold: true, size: 1, ..Default::default() }],
        ];
        let (plain, lines, links) = build_render_lines(&specs, 4);

        // Plain text equals the joined span text, newline-separated.
        assert_eq!(plain, "see docs\nX");
        assert_eq!(lines.len(), 2);

        // The link span produces one RenderLink over its display columns.
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].line, links[0].start_col, links[0].end_col), (0, 4, 8));
        assert_eq!(links[0].target, "guide.md");

        // Link span is underlined; bold/size span is bold + RGB.
        let link_style = lines[0].spans[1].style;
        assert!(link_style.add_modifier.contains(Modifier::UNDERLINED));
        let head = lines[1].spans[0].style;
        assert!(head.add_modifier.contains(Modifier::BOLD));
        assert_eq!(head.fg, Some(ratatui::style::Color::Rgb(255, 0, 0)));
    }
}
