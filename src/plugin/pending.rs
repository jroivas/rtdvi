use crate::Editor;

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
}

/// Result of replaying pending actions.
pub struct ApplyResult {
    /// Command names registered via `rtdvi_register_command`.
    pub new_commands: Vec<String>,
    /// File extensions this plugin declared itself manager for (e.g. `".lua"`).
    pub new_manager_exts: Vec<String>,
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

    let mut result = ApplyResult { new_commands: Vec::new(), new_manager_exts: Vec::new(), log_lines: Vec::new() };

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
            PendingAction::RegisterCommand { name } => {
                result.new_commands.push(name);
            }
            PendingAction::RegisterPluginManager { ext } => {
                result.new_manager_exts.push(ext);
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
