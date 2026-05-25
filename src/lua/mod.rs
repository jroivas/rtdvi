//! In-process Lua plugin engine (Phase A of Lua support).
//!
//! Lua plugins run natively inside jvim with access to a restricted `vim.*`
//! API subset. No WASM sandbox — security comes from only exposing safe API
//! surface (no `io.*`, `os.*`, `require` from filesystem).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, LuaOptions, RegistryKey, StdLib, Value};

use crate::plugin::pending::{apply_pending, PendingAction};
use crate::Editor;

// ── Internal types ────────────────────────────────────────────────────────────

struct LuaPlugin {
    /// Named ex-commands and auto-named key-handler stubs (e.g. "keyhandler_0").
    commands: HashMap<String, RegistryKey>,
}

#[derive(Default)]
struct LuaState {
    // Mutations accumulated during a Lua call; drained and applied after.
    pending: Vec<PendingAction>,

    // Editor snapshot — populated before each Lua invocation.
    active_buffer_id: Option<u32>,
    active_window_id: Option<u32>,
    cursor_cache: HashMap<u32, (usize, usize)>,
    line_cache: HashMap<u32, Vec<String>>,
    options: HashMap<String, String>,

    // Accumulated while loading a plugin; moved into LuaPlugin after exec().
    loading_commands: Vec<(String, RegistryKey)>,
    loading_keybinds: Vec<PendingAction>,
    loading_plugin: String,
    handler_counter: u32,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct LuaEngine {
    lua: Lua,
    plugins: HashMap<String, LuaPlugin>,
    state: Rc<RefCell<LuaState>>,
}

impl Default for LuaEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LuaEngine {
    pub fn new() -> Self {
        let state: Rc<RefCell<LuaState>> = Rc::new(RefCell::new(LuaState::default()));
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())
            .expect("Lua init");
        let mut engine = Self { lua, plugins: HashMap::new(), state };
        engine.setup_vim_api().expect("vim API setup");
        engine
    }

    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Load a Lua plugin from source text. Key bindings are applied to the
    /// editor immediately; command registrations are stored internally.
    pub fn load(&mut self, editor: &mut Editor, name: &str, content: &str) -> Result<(), String> {
        {
            let mut st = self.state.borrow_mut();
            st.loading_plugin = name.to_string();
            st.loading_commands.clear();
            st.loading_keybinds.clear();
        }
        self.snapshot_editor(editor);

        self.lua
            .load(content)
            .set_name(name)
            .exec()
            .map_err(|e| e.to_string())?;

        let (commands, pending) = {
            let mut st = self.state.borrow_mut();
            let cmds: HashMap<String, RegistryKey> =
                st.loading_commands.drain(..).collect();
            // Combine keybinds + any notify/print calls from top-level script.
            let mut pend: Vec<PendingAction> = st.pending.drain(..).collect();
            pend.extend(st.loading_keybinds.drain(..));
            (cmds, pend)
        };

        apply_pending(editor, pending, name);
        self.plugins.insert(name.to_string(), LuaPlugin { commands });
        Ok(())
    }

    /// Dispatch a command call into a loaded Lua plugin.
    pub fn run_command(
        &mut self,
        editor: &mut Editor,
        plugin: &str,
        cmd: &str,
        _args_json: &str,
    ) {
        self.snapshot_editor(editor);

        let key = match self.plugins.get(plugin).and_then(|p| p.commands.get(cmd)) {
            Some(k) => k,
            None => {
                tracing::warn!("[lua:{plugin}] command {cmd:?} not registered");
                return;
            }
        };

        let func: mlua::Function = match self.lua.registry_value(key) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("[lua:{plugin}] registry_value error: {e}");
                return;
            }
        };

        // Call with an empty opts table (Neovim user-command convention).
        let opts = match self.lua.create_table() {
            Ok(t) => t,
            Err(_) => return,
        };
        if let Err(e) = func.call::<()>(opts) {
            tracing::warn!("[lua:{plugin}] command {cmd:?} error: {e}");
        }

        let pending = std::mem::take(&mut self.state.borrow_mut().pending);
        apply_pending(editor, pending, plugin);
    }

    /// Remove a plugin and free its Lua registry entries.
    pub fn unload(&mut self, name: &str) {
        if let Some(plugin) = self.plugins.remove(name) {
            for (_, key) in plugin.commands {
                let _ = self.lua.remove_registry_value(key);
            }
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn snapshot_editor(&self, editor: &Editor) {
        let mut st = self.state.borrow_mut();
        st.active_buffer_id = editor.active_buffer_id().map(|id| id.0);
        st.active_window_id = editor.active_window().map(|w| w.id.0);

        st.cursor_cache.clear();
        for (id, win) in &editor.windows {
            st.cursor_cache.insert(id.0, (win.cursor.row, win.cursor.col));
        }

        st.line_cache.clear();
        for (id, buf) in &editor.buffers {
            let lines: Vec<String> =
                (0..buf.line_count()).map(|i| buf.line(i).to_string()).collect();
            st.line_cache.insert(id.0, lines);
        }

        st.options.clear();
        let opts = &editor.config.options;
        st.options.insert("tab_width".into(), opts.tab_width.to_string());
        st.options.insert("expandtab".into(), opts.expandtab.to_string());
        st.options.insert("number".into(), opts.number.to_string());
        st.options.insert("leader".into(), opts.leader.clone());
    }

    fn setup_vim_api(&mut self) -> mlua::Result<()> {
        let lua = &self.lua;

        // ── vim.notify(msg) ───────────────────────────────────────────────────
        let s = self.state.clone();
        let notify_fn = lua.create_function(move |_, msg: String| {
            s.borrow_mut().pending.push(PendingAction::SetStatus(msg));
            Ok(())
        })?;

        // ── print override ────────────────────────────────────────────────────
        let s = self.state.clone();
        let print_fn = lua.create_function(move |_, args: mlua::MultiValue| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string_lossy(),
                    other => format!("{other:?}"),
                })
                .collect();
            s.borrow_mut().pending.push(PendingAction::Log(parts.join("\t")));
            Ok(())
        })?;

        // ── vim.api.nvim_create_user_command(name, fn, opts) ──────────────────
        let s = self.state.clone();
        let create_cmd_fn =
            lua.create_function(move |lua, (name, func, _opts): (String, Value, Value)| {
                if let Value::Function(f) = func {
                    let key = lua.create_registry_value(f)?;
                    s.borrow_mut().loading_commands.push((name, key));
                }
                Ok(())
            })?;

        // ── vim.keymap.set(mode, lhs, rhs, opts) ─────────────────────────────
        let s = self.state.clone();
        let keymap_set_fn = lua.create_function(
            move |lua, (mode, lhs, rhs, _opts): (String, String, Value, Value)| {
                if let Value::Function(f) = rhs {
                    let mut st = s.borrow_mut();
                    let handler = format!("keyhandler_{}", st.handler_counter);
                    st.handler_counter += 1;
                    let key = lua.create_registry_value(f)?;
                    let plugin = st.loading_plugin.clone();
                    st.loading_commands.push((handler.clone(), key));
                    st.loading_keybinds.push(PendingAction::BindKey {
                        mode,
                        keys: lhs,
                        plugin_name: plugin,
                        function: handler,
                    });
                }
                Ok(())
            },
        )?;

        // ── vim.api.nvim_get_current_buf() ────────────────────────────────────
        let s = self.state.clone();
        let get_buf_fn = lua.create_function(move |_, ()| {
            Ok(s.borrow().active_buffer_id.map(|id| id as i64).unwrap_or(-1))
        })?;

        // ── vim.api.nvim_get_current_win() ────────────────────────────────────
        let s = self.state.clone();
        let get_win_fn = lua.create_function(move |_, ()| {
            Ok(s.borrow().active_window_id.map(|id| id as i64).unwrap_or(-1))
        })?;

        // ── vim.api.nvim_buf_get_lines(buf, start, end, strict) ───────────────
        let s = self.state.clone();
        let get_lines_fn = lua.create_function(
            move |lua, (buf, start, end, _strict): (i64, i64, i64, Value)| {
                let st = s.borrow();
                let tbl = lua.create_table()?;
                if let Some(lines) = st.line_cache.get(&(buf as u32)) {
                    let s_idx = start.max(0) as usize;
                    let e_idx = if end < 0 {
                        lines.len()
                    } else {
                        (end as usize).min(lines.len())
                    };
                    for (i, line) in lines[s_idx..e_idx].iter().enumerate() {
                        tbl.set(i + 1, line.as_str())?;
                    }
                }
                Ok(tbl)
            },
        )?;

        // ── vim.api.nvim_win_get_cursor(win) → {row, col} ────────────────────
        // Neovim convention: row is 1-indexed, col is 0-indexed.
        let s = self.state.clone();
        let get_cursor_fn = lua.create_function(move |lua, win: i64| {
            let st = s.borrow();
            let (row, col) =
                st.cursor_cache.get(&(win as u32)).copied().unwrap_or((0, 0));
            let tbl = lua.create_table()?;
            tbl.set(1, (row + 1) as i64)?;
            tbl.set(2, col as i64)?;
            Ok(tbl)
        })?;

        // ── vim.api table — known functions + __index fallback for the rest ───
        let vim_api = lua.create_table()?;
        vim_api.set("nvim_create_user_command", create_cmd_fn)?;
        vim_api.set("nvim_get_current_buf", get_buf_fn)?;
        vim_api.set("nvim_get_current_win", get_win_fn)?;
        vim_api.set("nvim_buf_get_lines", get_lines_fn)?;
        vim_api.set("nvim_win_get_cursor", get_cursor_fn)?;

        let api_meta = lua.create_table()?;
        let fallback = lua.create_function(|lua, (_t, key): (mlua::Table, String)| {
            tracing::debug!("vim.api.{key}: not implemented (no-op)");
            lua.create_function(|_, _: mlua::MultiValue| Ok(()))
        })?;
        api_meta.set("__index", fallback)?;
        let _ = vim_api.set_metatable(Some(api_meta));

        // ── vim.keymap table ──────────────────────────────────────────────────
        let keymap_tbl = lua.create_table()?;
        keymap_tbl.set("set", keymap_set_fn)?;

        // ── vim.o options table — __index reads from snapshot ─────────────────
        let s = self.state.clone();
        let opts_meta = lua.create_table()?;
        let opts_index_fn =
            lua.create_function(move |_, (_t, key): (mlua::Table, String)| {
                Ok(s.borrow().options.get(&key).cloned().unwrap_or_default())
            })?;
        opts_meta.set("__index", opts_index_fn)?;
        let vim_o = lua.create_table()?;
        let _ = vim_o.set_metatable(Some(opts_meta));

        // ── vim global ────────────────────────────────────────────────────────
        let vim_tbl = lua.create_table()?;
        vim_tbl.set("notify", notify_fn)?;
        vim_tbl.set("api", vim_api)?;
        vim_tbl.set("keymap", keymap_tbl)?;
        vim_tbl.set("o", vim_o)?;

        lua.globals().set("vim", vim_tbl)?;
        lua.globals().set("print", print_fn)?;

        Ok(())
    }
}
