//! mlua-wasm — jvim plugin manager for Lua plugins.
//!
//! ## Build
//!
//!   cd examples/plugin/mlua-wasm
//!   cargo build --target wasm32-unknown-unknown --release
//!   cp target/wasm32-unknown-unknown/release/mlua_wasm.wasm \
//!      ~/.local/jvim/plugins/mlua-wasm.wasm
//!
//! ## Enable in ~/.config/jvim/config.toml
//!
//!   plugins = [
//!     "mlua-wasm",      # load manager first
//!     "hello.lua",      # then .lua plugins
//!   ]
//!
//! ## Lua API (neovim-compatible subset)
//!
//!   vim.notify(msg)                                    -- set status / log
//!   print(msg)                                         -- log
//!   vim.keymap.set(mode, lhs, fn, opts)                -- bind key
//!   vim.api.nvim_set_keymap(mode, lhs, rhs, opts)
//!   vim.api.nvim_create_user_command(name, fn, opts)   -- register command
//!   vim.api.nvim_get_current_buf()
//!   vim.api.nvim_buf_get_lines(buf, start, end, strict)
//!   vim.api.nvim_buf_set_lines(buf, start, end, strict, lines)
//!   vim.api.nvim_get_current_win()
//!   vim.api.nvim_win_get_cursor(win)
//!   vim.api.nvim_win_set_cursor(win, {row, col})
//!   vim.cmd(cmd)  /  vim.api.nvim_command(cmd)
//!   vim.o.<option>                                     -- read editor options

use std::alloc::Layout;
use std::collections::HashMap;
use std::slice;

use mlua::prelude::*;

// ── jvim host imports ─────────────────────────────────────────────────────────

#[link(wasm_import_module = "jvim")]
extern "C" {
    fn jvim_log(ptr: i32, len: i32);
    fn jvim_set_status(ptr: i32, len: i32);
    fn jvim_register_command(ptr: i32, len: i32) -> i32;
    fn jvim_register_plugin_manager(ptr: i32, len: i32) -> i32;
    fn jvim_bind_key(
        mode_ptr: i32, mode_len: i32,
        keys_ptr: i32, keys_len: i32,
        fn_ptr:   i32, fn_len:   i32,
    ) -> i32;
    fn jvim_active_buffer_id() -> i32;
    fn jvim_active_window_id() -> i32;
    fn jvim_line_count(buf_id: i32) -> i32;
    fn jvim_get_line(buf_id: i32, row: i32, out_ptr: i32, max_len: i32) -> i32;
    fn jvim_insert_text(buf_id: i32, char_pos: i32, ptr: i32, len: i32) -> i32;
    fn jvim_delete_text(buf_id: i32, start: i32, end: i32) -> i32;
    fn jvim_get_cursor(win_id: i32) -> i64;
    fn jvim_set_cursor(win_id: i32, row: i32, col: i32) -> i32;
    fn jvim_get_option_str(
        key_ptr: i32, key_len: i32,
        out_ptr: i32, max_len: i32,
    ) -> i32;
}

// ── Memory exports ────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    if size <= 0 { return 0; }
    match Layout::from_size_align(size as usize, 8) {
        Ok(layout) => unsafe { std::alloc::alloc(layout) as i32 },
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: i32, size: i32) {
    if ptr == 0 || size <= 0 { return; }
    if let Ok(layout) = Layout::from_size_align(size as usize, 8) {
        unsafe { std::alloc::dealloc(ptr as *mut u8, layout) }
    }
}

// ── Host helpers ──────────────────────────────────────────────────────────────

fn host_log(msg: &str) {
    unsafe { jvim_log(msg.as_ptr() as i32, msg.len() as i32) }
}

fn host_set_status(msg: &str) {
    unsafe { jvim_set_status(msg.as_ptr() as i32, msg.len() as i32) }
}

fn host_register_command(name: &str) {
    unsafe { jvim_register_command(name.as_ptr() as i32, name.len() as i32) };
}

fn host_bind_key(mode: &str, keys: &str, func: &str) {
    unsafe {
        jvim_bind_key(
            mode.as_ptr() as i32, mode.len() as i32,
            keys.as_ptr() as i32, keys.len() as i32,
            func.as_ptr() as i32, func.len() as i32,
        )
    };
}

fn read_str_from_ptr(ptr: i32, len: i32) -> &'static str {
    if len <= 0 { return ""; }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    std::str::from_utf8(bytes).unwrap_or("")
}

fn host_get_option(key: &str) -> String {
    let mut buf = vec![0u8; 256];
    let n = unsafe {
        jvim_get_option_str(
            key.as_ptr() as i32, key.len() as i32,
            buf.as_mut_ptr() as i32, buf.len() as i32,
        )
    };
    if n > 0 {
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    } else {
        String::new()
    }
}

// ── Per-plugin state ──────────────────────────────────────────────────────────

struct PluginData {
    /// Lua functions registered for commands. Key = "plugin_name.cmd_name".
    commands: HashMap<String, LuaRegistryKey>,
    /// Lua functions bound to keys. Key = "plugin_name.handler_N".
    handlers: HashMap<String, LuaRegistryKey>,
    /// Lua functions called on every editor event.
    event_listeners: Vec<LuaRegistryKey>,
}

// ── Global state (single-threaded WASM) ──────────────────────────────────────

struct State {
    lua: Lua,
    plugins: HashMap<String, PluginData>,
    current_plugin: String,
    handler_seq: u32,
}

// WASM is single-threaded: a plain static mut is safe here.
// thread_local! on wasm32-unknown-emscripten requires emscripten's TLS
// initialisation (via __tls_base) which never runs when wasmtime calls an
// exported function directly (no emscripten startup sequence).
static mut STATE: Option<State> = None;

fn with_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    unsafe {
        f(STATE.as_mut().expect("mlua-wasm not initialised"))
    }
}

// ── Lua API setup ─────────────────────────────────────────────────────────────

fn setup_vim_api(lua: &Lua) -> LuaResult<()> {
    // print(msg) → host log
    lua.globals().set(
        "print",
        lua.create_function(|_, msg: String| {
            host_log(&msg);
            Ok(())
        })?,
    )?;

    let vim = lua.create_table()?;

    // vim.notify(msg [, level])
    vim.set(
        "notify",
        lua.create_function(|_, (msg, _level): (String, Option<LuaValue>)| {
            host_set_status(&msg);
            Ok(())
        })?,
    )?;

    // vim.cmd(str)
    let cmd_fn = lua.create_function(|_, _cmd: String| {
        // ex command passthrough not yet implemented
        host_log("vim.cmd: not yet implemented");
        Ok(())
    })?;
    vim.set("cmd", cmd_fn)?;

    // vim.o — read editor options via __index
    let vim_o = lua.create_table()?;
    let vim_o_meta = lua.create_table()?;
    vim_o_meta.set(
        "__index",
        lua.create_function(|_, (_t, key): (LuaTable, String)| {
            Ok(host_get_option(&key))
        })?,
    )?;
    let _ = vim_o.set_metatable(Some(vim_o_meta));
    vim.set("o", vim_o)?;

    // vim.log.levels
    let log_tbl = lua.create_table()?;
    let levels = lua.create_table()?;
    levels.set("INFO", 1)?;
    levels.set("WARN", 2)?;
    levels.set("ERROR", 3)?;
    log_tbl.set("levels", levels)?;
    vim.set("log", log_tbl)?;

    // vim.keymap.set(mode, lhs, rhs, opts)
    let keymap = lua.create_table()?;
    keymap.set(
        "set",
        lua.create_function(|lua, (mode, lhs, rhs, _opts): (String, String, LuaValue, Option<LuaTable>)| {
            with_state(|s| {
                let plugin = s.current_plugin.clone();
                match rhs {
                    LuaValue::Function(f) => {
                        let seq = s.handler_seq;
                        s.handler_seq += 1;
                        let handler_key = format!("handler_{seq}");
                        let full_key = format!("{plugin}.{handler_key}");
                        let rk = lua.create_registry_value(f)?;
                        s.plugins.entry(plugin.clone()).or_insert_with(|| PluginData {
                            commands: HashMap::new(),
                            handlers: HashMap::new(),
                            event_listeners: Vec::new(),
                        }).handlers.insert(full_key.clone(), rk);
                        // Expand <leader> using current option
                        let leader = host_get_option("leader");
                        let expanded_lhs = lhs.replace("<leader>", &leader);
                        host_bind_key(&mode, &expanded_lhs, &full_key);
                    }
                    LuaValue::String(s_val) => {
                        // RHS is a string ex command — bind directly as builtin
                        let cmd = s_val.to_str().map(|b| b.to_string()).unwrap_or_default();
                        let leader = host_get_option("leader");
                        let expanded_lhs = lhs.replace("<leader>", &leader);
                        // Use a synthetic handler that runs the ex command
                        let seq = s.handler_seq;
                        s.handler_seq += 1;
                        let handler_key = format!("handler_{seq}");
                        let full_key = format!("{plugin}.{handler_key}");
                        let f = lua.create_function(move |_, ()| {
                            host_set_status(&cmd);
                            Ok(())
                        })?;
                        let rk = lua.create_registry_value(f)?;
                        s.plugins.entry(plugin.clone()).or_insert_with(|| PluginData {
                            commands: HashMap::new(),
                            handlers: HashMap::new(),
                            event_listeners: Vec::new(),
                        }).handlers.insert(full_key.clone(), rk);
                        host_bind_key(&mode, &expanded_lhs, &full_key);
                    }
                    _ => {}
                }
                Ok(())
            })
        })?,
    )?;
    vim.set("keymap", keymap)?;

    // vim.api — neovim-compatible subset with __index stub for unimplemented fns
    let api = lua.create_table()?;

    api.set(
        "nvim_set_keymap",
        lua.create_function(|lua, (mode, lhs, rhs, _opts): (String, String, String, Option<LuaTable>)| {
            // Forward to vim.keymap.set
            with_state(|s| -> LuaResult<()> {
                let plugin = s.current_plugin.clone();
                let seq = s.handler_seq;
                s.handler_seq += 1;
                let handler_key = format!("handler_{seq}");
                let full_key = format!("{plugin}.{handler_key}");
                let cmd_copy = rhs.clone();
                let f = lua.create_function(move |_, ()| {
                    host_set_status(&cmd_copy);
                    Ok(())
                })?;
                let rk = lua.create_registry_value(f)?;
                s.plugins.entry(plugin).or_insert_with(|| PluginData {
                    commands: HashMap::new(),
                    handlers: HashMap::new(),
                    event_listeners: Vec::new(),
                }).handlers.insert(full_key.clone(), rk);
                let leader = host_get_option("leader");
                let expanded = lhs.replace("<leader>", &leader);
                host_bind_key(&mode, &expanded, &full_key);
                Ok(())
            })
        })?,
    )?;

    api.set(
        "nvim_create_user_command",
        lua.create_function(|lua, (name, cb, _opts): (String, LuaFunction, Option<LuaTable>)| {
            with_state(|s| -> LuaResult<()> {
                let plugin = s.current_plugin.clone();
                let full_key = format!("{plugin}.{name}");
                let rk = lua.create_registry_value(cb)?;
                s.plugins.entry(plugin).or_insert_with(|| PluginData {
                    commands: HashMap::new(),
                    handlers: HashMap::new(),
                    event_listeners: Vec::new(),
                }).commands.insert(full_key.clone(), rk);
                host_register_command(&full_key);
                Ok(())
            })
        })?,
    )?;

    api.set(
        "nvim_get_current_buf",
        lua.create_function(|_, ()| {
            Ok(unsafe { jvim_active_buffer_id() })
        })?,
    )?;

    api.set(
        "nvim_get_current_win",
        lua.create_function(|_, ()| {
            Ok(unsafe { jvim_active_window_id() })
        })?,
    )?;

    api.set(
        "nvim_buf_get_lines",
        lua.create_function(|lua, (buf_id, start, end, _strict): (i32, i32, i32, bool)| {
            let line_count = unsafe { jvim_line_count(buf_id) };
            if line_count < 0 { return Ok(lua.create_table()?); }
            let from = if start < 0 { (line_count + start).max(0) } else { start } as usize;
            let to = if end < 0 { (line_count + end + 1).max(0) } else { end.min(line_count) } as usize;
            let result = lua.create_table()?;
            let mut buf = vec![0u8; 8192];
            for row in from..to {
                let n = unsafe {
                    jvim_get_line(buf_id, row as i32, buf.as_mut_ptr() as i32, buf.len() as i32)
                };
                let line = if n > 0 {
                    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
                } else {
                    String::new()
                };
                result.push(line)?;
            }
            Ok(result)
        })?,
    )?;

    api.set(
        "nvim_buf_set_lines",
        lua.create_function(
            |_, (buf_id, start, end, _strict, lines): (i32, i32, i32, bool, Vec<String>)| {
                let line_count = unsafe { jvim_line_count(buf_id) };
                if line_count < 0 { return Ok(()); }
                // Delete the range [start, end)
                unsafe { jvim_delete_text(buf_id, start, end.min(line_count)) };
                // Insert new lines at position `start`
                let text = lines.join("\n");
                unsafe {
                    jvim_insert_text(
                        buf_id,
                        start,
                        text.as_ptr() as i32,
                        text.len() as i32,
                    )
                };
                Ok(())
            },
        )?,
    )?;

    api.set(
        "nvim_win_get_cursor",
        lua.create_function(|lua, win_id: i32| {
            let packed = unsafe { jvim_get_cursor(win_id) };
            if packed < 0 { return Ok(lua.create_table()?); }
            let row = ((packed >> 32) as i32) + 1; // 1-based like neovim
            let col = (packed & 0xffffffff) as i32;
            let t = lua.create_table()?;
            t.push(row)?;
            t.push(col)?;
            Ok(t)
        })?,
    )?;

    api.set(
        "nvim_win_set_cursor",
        lua.create_function(|_, (win_id, pos): (i32, LuaTable)| {
            let row: i32 = pos.get(1).unwrap_or(1);
            let col: i32 = pos.get(2).unwrap_or(0);
            unsafe { jvim_set_cursor(win_id, row - 1, col) }; // 0-based internally
            Ok(())
        })?,
    )?;

    let cmd_fn2 = lua.create_function(|_, _cmd: String| {
        host_log("vim.api.nvim_command: not yet implemented");
        Ok(())
    })?;
    api.set("nvim_command", cmd_fn2)?;

    // Unimplemented stub via __index
    let api_meta = lua.create_table()?;
    api_meta.set(
        "__index",
        lua.create_function(|lua, (_t, key): (LuaTable, String)| {
            let msg = format!("vim.api.{key} not implemented");
            host_log(&msg);
            let noop: LuaFunction = lua.create_function(|_, _args: LuaMultiValue| Ok(()))?;
            Ok(noop)
        })?,
    )?;
    let _ = api.set_metatable(Some(api_meta));

    vim.set("api", api)?;
    lua.globals().set("vim", vim)?;
    Ok(())
}

// ── Plugin lifecycle ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn jvim_init(cfg_ptr: i32, cfg_len: i32) -> i32 {
    let _cfg = read_str_from_ptr(cfg_ptr, cfg_len);

    host_log("mlua-wasm: creating Lua state...");
    let lua = match Lua::new_with(mlua::StdLib::ALL_SAFE, mlua::LuaOptions::default()) {
        Ok(l) => l,
        Err(e) => {
            let msg = format!("mlua-wasm: Lua::new failed: {e}");
            host_set_status(&msg);
            host_log(&msg);
            return 1;
        }
    };
    host_log("mlua-wasm: Lua state created OK");

    if let Err(e) = setup_vim_api(&lua) {
        let msg = format!("mlua-wasm: setup_vim_api failed: {e}");
        host_set_status(&msg);
        host_log(&msg);
        return 1;
    }

    unsafe {
        STATE = Some(State {
            lua,
            plugins: HashMap::new(),
            current_plugin: String::new(),
            handler_seq: 0,
        });
    }

    let ext = ".lua";
    unsafe { jvim_register_plugin_manager(ext.as_ptr() as i32, ext.len() as i32) };
    host_log("mlua-wasm: ready (Lua 5.4 plugin manager)");
    0
}

#[no_mangle]
pub extern "C" fn load_plugin(
    name_ptr: i32, name_len: i32,
    content_ptr: i32, content_len: i32,
) -> i32 {
    let name = read_str_from_ptr(name_ptr, name_len).to_string();
    let content = read_str_from_ptr(content_ptr, content_len).to_string();

    with_state(|s| {
        s.current_plugin = name.clone();
        s.plugins.entry(name.clone()).or_insert_with(|| PluginData {
            commands: HashMap::new(),
            handlers: HashMap::new(),
            event_listeners: Vec::new(),
        });

        if let Err(e) = s.lua.load(&content).set_name(&name).exec() {
            let msg = format!("mlua-wasm: error loading {name:?}: {e}");
            host_log(&msg);
            return -1;
        }
        let msg = format!("mlua-wasm: loaded {name:?}");
        host_log(&msg);
        0
    })
}

#[no_mangle]
pub extern "C" fn unload_plugin(name_ptr: i32, name_len: i32) -> i32 {
    let name = read_str_from_ptr(name_ptr, name_len).to_string();
    with_state(|s| {
        s.plugins.remove(&name);
        let msg = format!("mlua-wasm: unloaded {name:?}");
        host_log(&msg);
        0
    })
}

#[no_mangle]
pub extern "C" fn run_command(
    name_ptr: i32, name_len: i32,
    args_ptr: i32, args_len: i32,
) -> i32 {
    // name is "plugin_name.cmd_or_handler_key"
    let name = read_str_from_ptr(name_ptr, name_len).to_string();
    let args = read_str_from_ptr(args_ptr, args_len).to_string();

    with_state(|s| {
        // Find the Lua function registered for this namespaced name.
        let rk = s.plugins.values()
            .find_map(|pd| {
                pd.commands.get(&name).or_else(|| pd.handlers.get(&name))
            });

        let rk = match rk {
            Some(k) => {
                // SAFETY: registry keys are valid as long as the Lua state lives
                // We need to clone the registry key — but LuaRegistryKey is not Clone.
                // Work around by looking it up via the Lua API.
                k as *const LuaRegistryKey
            }
            None => {
                let msg = format!("mlua-wasm: no handler for {name:?}");
                host_log(&msg);
                return -1;
            }
        };

        let result: LuaResult<()> = (|| {
            let f: LuaFunction = s.lua.registry_value(unsafe { &*rk })?;
            // Parse args JSON array into a Lua table
            f.call::<()>(args.clone())?;
            Ok(())
        })();

        if let Err(e) = result {
            let msg = format!("mlua-wasm: error in {name:?}: {e}");
            host_log(&msg);
            return -1;
        }
        0
    })
}

#[no_mangle]
pub extern "C" fn on_event(event_ptr: i32, event_len: i32) {
    let event_json = read_str_from_ptr(event_ptr, event_len).to_string();

    with_state(|s| {
        // Collect all registry key pointers first to avoid borrow issues.
        let listeners: Vec<*const LuaRegistryKey> = s.plugins.values()
            .flat_map(|pd| pd.event_listeners.iter().map(|rk| rk as *const LuaRegistryKey))
            .collect();

        for rk_ptr in listeners {
            let result: LuaResult<()> = (|| {
                let f: LuaFunction = s.lua.registry_value(unsafe { &*rk_ptr })?;
                f.call::<()>(event_json.clone())?;
                Ok(())
            })();
            if let Err(e) = result {
                let msg = format!("mlua-wasm: event handler error: {e}");
                host_log(&msg);
            }
        }
    });
}
