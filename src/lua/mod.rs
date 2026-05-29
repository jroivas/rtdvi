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

struct AutocmdEntry {
    /// Comma-separated event names or "*" for all.
    event: String,
    key: RegistryKey,
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
    buffer_names: HashMap<u32, String>,
    options: HashMap<String, String>,

    // Accumulated while loading a plugin; moved into LuaPlugin after exec().
    loading_commands: Vec<(String, RegistryKey)>,
    loading_keybinds: Vec<PendingAction>,
    loading_plugin: String,
    handler_counter: u32,

    // Autocmd registry (populated by nvim_create_autocmd).
    autocmds: Vec<AutocmdEntry>,
    augroup_counter: u32,
    autocmd_counter: u32,
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

    /// Fire editor events to registered autocmd callbacks.
    pub fn on_event(&mut self, editor: &mut Editor, event: &str) {
        self.snapshot_editor(editor);

        let matching: Vec<*const RegistryKey> = {
            let st = self.state.borrow();
            st.autocmds
                .iter()
                .filter(|ac| {
                    ac.event == "*"
                        || ac.event
                            .split(',')
                            .any(|e| e.trim().eq_ignore_ascii_case(event))
                })
                .map(|ac| &ac.key as *const RegistryKey)
                .collect()
        };

        for rk_ptr in matching {
            let result: mlua::Result<()> = (|| {
                let f: mlua::Function = self.lua.registry_value(unsafe { &*rk_ptr })?;
                let tbl = self.lua.create_table()?;
                tbl.set("event", event)?;
                f.call::<()>(tbl)?;
                Ok(())
            })();
            if let Err(e) = result {
                tracing::warn!("[lua] autocmd({event}) error: {e}");
            }
        }

        let pending = std::mem::take(&mut self.state.borrow_mut().pending);
        apply_pending(editor, pending, "autocmd");
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
        st.buffer_names.clear();
        for (id, buf) in &editor.buffers {
            let lines: Vec<String> =
                (0..buf.line_count()).map(|i| buf.line(i).to_string()).collect();
            st.line_cache.insert(id.0, lines);
            st.buffer_names.insert(id.0, buf.display_name());
        }

        st.options.clear();
        let opts = &editor.config.options;
        st.options.insert("tab_width".into(), opts.tab_width.to_string());
        st.options.insert("tabstop".into(), opts.tab_width.to_string());
        st.options.insert("shiftwidth".into(), opts.tab_width.to_string());
        st.options.insert("expandtab".into(), opts.expandtab.to_string());
        st.options.insert("number".into(), opts.number.to_string());
        st.options.insert("leader".into(), opts.leader.clone());
    }

    #[allow(clippy::too_many_lines)]
    fn setup_vim_api(&mut self) -> mlua::Result<()> {
        let lua = &self.lua;

        // ── vim.notify(msg [, level [, opts]]) ────────────────────────────────
        let s = self.state.clone();
        let notify_fn = lua.create_function(move |_, (msg, _level, _opts): (String, Value, Value)| {
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

        // ── vim.cmd(str | table) ──────────────────────────────────────────────
        let cmd_fn = lua.create_function(|_, _cmd: Value| {
            tracing::debug!("vim.cmd: not yet implemented");
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

        // ── vim.api.nvim_del_user_command(name) ───────────────────────────────
        let del_cmd_fn = lua.create_function(|_, _name: String| Ok(()))?;

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

        // ── vim.keymap.del(mode, lhs [, opts]) ────────────────────────────────
        let keymap_del_fn = lua.create_function(|_, _args: mlua::MultiValue| Ok(()))?;

        // ── vim.api.nvim_set_keymap(mode, lhs, rhs, opts) ────────────────────
        let s = self.state.clone();
        let set_keymap_fn = lua.create_function(
            move |lua, (mode, lhs, rhs, _opts): (String, String, String, Value)| {
                let mut st = s.borrow_mut();
                let handler = format!("keyhandler_{}", st.handler_counter);
                st.handler_counter += 1;
                let plugin = st.loading_plugin.clone();
                let f = lua.create_function(move |_, ()| {
                    tracing::debug!("keymap rhs string: {rhs}");
                    Ok(())
                })?;
                let key = lua.create_registry_value(f)?;
                st.loading_commands.push((handler.clone(), key));
                st.loading_keybinds.push(PendingAction::BindKey {
                    mode,
                    keys: lhs,
                    plugin_name: plugin,
                    function: handler,
                });
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

        // ── vim.api.nvim_buf_line_count(buf) ──────────────────────────────────
        let s = self.state.clone();
        let buf_line_count_fn = lua.create_function(move |_, buf: i64| {
            Ok(s.borrow()
                .line_cache
                .get(&(buf as u32))
                .map(|l| l.len() as i64)
                .unwrap_or(-1))
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

        // ── vim.api.nvim_buf_set_lines(buf, start, end, strict, lines) ────────
        let s = self.state.clone();
        let set_lines_fn = lua.create_function(
            move |_, (buf, start, end, _strict, lines): (i64, i64, i64, Value, Vec<String>)| {
                s.borrow_mut().pending.push(PendingAction::SetLines {
                    buffer_id: buf as u32,
                    start,
                    end,
                    lines,
                });
                Ok(())
            },
        )?;

        // ── vim.api.nvim_buf_get_name(buf) ────────────────────────────────────
        let s = self.state.clone();
        let buf_get_name_fn = lua.create_function(move |_, buf: i64| {
            Ok(s.borrow()
                .buffer_names
                .get(&(buf as u32))
                .cloned()
                .unwrap_or_default())
        })?;

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

        // ── vim.api.nvim_win_set_cursor(win, {row, col}) ──────────────────────
        let s = self.state.clone();
        let set_cursor_fn =
            lua.create_function(move |_, (win, pos): (i64, mlua::Table)| {
                let row: i64 = pos.get(1).unwrap_or(1);
                let col: i64 = pos.get(2).unwrap_or(0);
                s.borrow_mut().pending.push(PendingAction::SetCursor {
                    window_id: win as u32,
                    row: (row - 1).max(0) as usize,
                    col: col as usize,
                });
                Ok(())
            })?;

        // ── vim.api.nvim_command(cmd) ─────────────────────────────────────────
        let nvim_cmd_fn = lua.create_function(|_, _cmd: String| {
            tracing::debug!("vim.api.nvim_command: not yet implemented");
            Ok(())
        })?;

        // ── vim.api.nvim_create_namespace(name) ───────────────────────────────
        let create_ns_fn = lua.create_function(|_, _name: String| Ok(0_i64))?;

        // ── vim.api.nvim_create_augroup(name, opts) ───────────────────────────
        let s = self.state.clone();
        let create_augroup_fn =
            lua.create_function(move |_, (_name, _opts): (String, Value)| {
                let mut st = s.borrow_mut();
                st.augroup_counter += 1;
                Ok(st.augroup_counter as i64)
            })?;

        // ── vim.api.nvim_create_autocmd(event, opts) ──────────────────────────
        let s = self.state.clone();
        let create_autocmd_fn = lua.create_function(
            move |lua, (event, opts): (Value, mlua::Table)| {
                let event_str = match &event {
                    Value::String(es) => es.to_string_lossy(),
                    Value::Table(t) => {
                        let names: Vec<String> =
                            t.clone().sequence_values::<String>().flatten().collect();
                        names.join(",")
                    }
                    _ => "*".to_string(),
                };
                if let Ok(Value::Function(f)) = opts.get::<Value>("callback") {
                    let rk = lua.create_registry_value(f)?;
                    let mut st = s.borrow_mut();
                    st.autocmd_counter += 1;
                    let id = st.autocmd_counter;
                    st.autocmds.push(AutocmdEntry { event: event_str, key: rk });
                    return Ok(id as i64);
                }
                Ok(0)
            },
        )?;

        // ── vim.api.nvim_clear_autocmds(opts) ────────────────────────────────
        let clear_autocmds_fn = lua.create_function(|_, _opts: Value| Ok(()))?;

        // ── vim.api.nvim_set_hl(ns, name, opts) ──────────────────────────────
        let set_hl_fn = lua.create_function(|_, _args: mlua::MultiValue| {
            tracing::debug!("vim.api.nvim_set_hl: not yet implemented");
            Ok(())
        })?;

        // ── vim.api.nvim_exec2 / nvim_exec stubs ─────────────────────────────
        let exec_fn = lua.create_function(|_, _args: mlua::MultiValue| {
            tracing::debug!("vim.api.nvim_exec2: not yet implemented");
            Ok(mlua::Value::Nil)
        })?;

        // ── vim.api table — known functions + __index fallback ────────────────
        let vim_api = lua.create_table()?;
        vim_api.set("nvim_create_user_command", create_cmd_fn)?;
        vim_api.set("nvim_del_user_command", del_cmd_fn)?;
        vim_api.set("nvim_set_keymap", set_keymap_fn)?;
        vim_api.set("nvim_get_current_buf", get_buf_fn)?;
        vim_api.set("nvim_get_current_win", get_win_fn)?;
        vim_api.set("nvim_buf_line_count", buf_line_count_fn)?;
        vim_api.set("nvim_buf_get_lines", get_lines_fn)?;
        vim_api.set("nvim_buf_set_lines", set_lines_fn)?;
        vim_api.set("nvim_buf_get_name", buf_get_name_fn)?;
        vim_api.set("nvim_win_get_cursor", get_cursor_fn)?;
        vim_api.set("nvim_win_set_cursor", set_cursor_fn)?;
        vim_api.set("nvim_command", nvim_cmd_fn)?;
        vim_api.set("nvim_create_namespace", create_ns_fn)?;
        vim_api.set("nvim_create_augroup", create_augroup_fn)?;
        vim_api.set("nvim_create_autocmd", create_autocmd_fn)?;
        vim_api.set("nvim_clear_autocmds", clear_autocmds_fn)?;
        vim_api.set("nvim_set_hl", set_hl_fn)?;
        vim_api.set("nvim_exec2", exec_fn.clone())?;
        vim_api.set("nvim_exec", exec_fn)?;

        let api_meta = lua.create_table()?;
        let fallback = lua.create_function(|lua, (_t, key): (mlua::Table, String)| {
            tracing::debug!("vim.api.{key}: not implemented (no-op)");
            lua.create_function(|_, _: mlua::MultiValue| Ok(mlua::Value::Nil))
        })?;
        api_meta.set("__index", fallback)?;
        let _ = vim_api.set_metatable(Some(api_meta));

        // ── vim.keymap table ──────────────────────────────────────────────────
        let keymap_tbl = lua.create_table()?;
        keymap_tbl.set("set", keymap_set_fn)?;
        keymap_tbl.set("del", keymap_del_fn)?;

        // ── vim.o / vim.opt options tables ────────────────────────────────────
        let s = self.state.clone();
        let opts_index_fn =
            lua.create_function(move |_, (_t, key): (mlua::Table, String)| {
                Ok(s.borrow().options.get(&key).cloned().unwrap_or_default())
            })?;

        let opts_meta = lua.create_table()?;
        opts_meta.set("__index", opts_index_fn.clone())?;
        let vim_o = lua.create_table()?;
        let _ = vim_o.set_metatable(Some(opts_meta));

        // vim.opt — reads like vim.o; __newindex queues SetOption
        let s = self.state.clone();
        let opt_newindex_fn = lua.create_function(
            move |_, (_t, key, val): (mlua::Table, String, Value)| {
                let value_str = match &val {
                    Value::Boolean(b) => b.to_string(),
                    Value::Integer(i) => i.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.to_string_lossy(),
                    _ => return Ok(()),
                };
                s.borrow_mut()
                    .pending
                    .push(PendingAction::SetOption { name: key, value: value_str });
                Ok(())
            },
        )?;
        let opt_meta = lua.create_table()?;
        opt_meta.set("__index", opts_index_fn)?;
        opt_meta.set("__newindex", opt_newindex_fn)?;
        let vim_opt = lua.create_table()?;
        let _ = vim_opt.set_metatable(Some(opt_meta));

        // ── vim.fn basic functions ────────────────────────────────────────────
        let s = self.state.clone();
        let fn_expand = lua.create_function(move |_, expr: String| {
            let st = s.borrow();
            if expr == "%" || expr == "%:p" {
                let buf_id = st.active_buffer_id.unwrap_or(0);
                return Ok(st.buffer_names.get(&buf_id).cloned().unwrap_or_default());
            }
            Ok(String::new())
        })?;
        let fn_has = lua.create_function(|_, _: String| Ok(0_i64))?;
        let fn_exists = lua.create_function(|_, _: String| Ok(0_i64))?;
        let fn_getcwd = lua.create_function(|_, ()| {
            Ok(std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default())
        })?;
        let fn_printf = lua.create_function(|_, (fmt, _rest): (String, mlua::MultiValue)| {
            Ok(fmt)
        })?;
        let fn_shellescape = lua.create_function(|_, s: String| Ok(s))?;
        let fn_fnameescape = lua.create_function(|_, s: String| Ok(s))?;

        let vim_fn = lua.create_table()?;
        vim_fn.set("expand", fn_expand)?;
        vim_fn.set("has", fn_has)?;
        vim_fn.set("exists", fn_exists)?;
        vim_fn.set("getcwd", fn_getcwd)?;
        vim_fn.set("printf", fn_printf)?;
        vim_fn.set("shellescape", fn_shellescape)?;
        vim_fn.set("fnameescape", fn_fnameescape)?;

        let fn_meta = lua.create_table()?;
        fn_meta.set(
            "__index",
            lua.create_function(|lua, (_t, key): (mlua::Table, String)| {
                tracing::debug!("vim.fn.{key}: not implemented (no-op)");
                lua.create_function(|_, _: mlua::MultiValue| Ok(mlua::Value::Nil))
            })?,
        )?;
        let _ = vim_fn.set_metatable(Some(fn_meta));

        // ── vim.log.levels ────────────────────────────────────────────────────
        let log_tbl = lua.create_table()?;
        let levels = lua.create_table()?;
        levels.set("TRACE", 0)?;
        levels.set("DEBUG", 1)?;
        levels.set("INFO", 2)?;
        levels.set("WARN", 3)?;
        levels.set("ERROR", 4)?;
        log_tbl.set("levels", levels)?;

        // ── vim global ────────────────────────────────────────────────────────
        let vim_tbl = lua.create_table()?;
        vim_tbl.set("notify", notify_fn)?;
        vim_tbl.set("api", vim_api)?;
        vim_tbl.set("keymap", keymap_tbl)?;
        vim_tbl.set("o", vim_o)?;
        vim_tbl.set("opt", vim_opt)?;
        vim_tbl.set("fn", vim_fn)?;
        vim_tbl.set("cmd", cmd_fn)?;
        vim_tbl.set("log", log_tbl)?;
        // Variable scopes — simple tables; plugins can store into them freely.
        vim_tbl.set("g", lua.create_table()?)?;
        vim_tbl.set("b", lua.create_table()?)?;
        vim_tbl.set("w", lua.create_table()?)?;
        vim_tbl.set("t", lua.create_table()?)?;

        lua.globals().set("vim", vim_tbl)?;
        lua.globals().set("print", print_fn)?;

        Ok(())
    }
}
