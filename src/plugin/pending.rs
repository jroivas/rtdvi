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
    SetCursor { window_id: u32, row: usize, col: usize },
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
    /// Command names registered via `jvim_register_command`.
    pub new_commands: Vec<String>,
    /// File extensions this plugin declared itself manager for (e.g. `".lua"`).
    pub new_manager_exts: Vec<String>,
}

/// Replay a batch of pending actions onto the editor.
pub fn apply_pending(
    editor: &mut Editor,
    actions: Vec<PendingAction>,
    plugin_name: &str,
) -> ApplyResult {
    use crate::buffer::BufferId;
    use crate::window::WindowId;

    let mut result = ApplyResult { new_commands: Vec::new(), new_manager_exts: Vec::new() };

    for action in actions {
        match action {
            PendingAction::Log(msg) => {
                tracing::info!("[plugin:{plugin_name}] {msg}");
            }
            PendingAction::SetStatus(msg) => {
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
            PendingAction::SetCursor { window_id, row, col } => {
                if let Some(win) = editor.windows.get_mut(&WindowId(window_id)) {
                    win.cursor.row = row;
                    win.cursor.col = col;
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
