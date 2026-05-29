pub mod abi;
pub mod config;
pub mod loader;
pub mod pending;
pub mod runtime;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use self::runtime::{Linker, Memory, Module, Store, TypedFunc};

use crate::command::{CommandError, ExArgs, ExCommand};
use crate::event::{Event, Listener};
use crate::Editor;

use self::config::PluginEntry;
use self::pending::apply_pending;
pub use self::pending::{ApplyResult, PendingAction};

/// Open a horizontal split with a scratch buffer showing plugin log output.
/// Does nothing if both `header` and `logs` are empty.
fn show_plugin_error_log(editor: &mut Editor, plugin_name: &str, header: &str, logs: &[String]) {
    if header.is_empty() && logs.is_empty() {
        return;
    }
    let buf_id = editor.open_scratch();
    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
        buf.set_name(format!("[Plugin: {plugin_name}]"));
        let mut content = String::new();
        if !header.is_empty() {
            content.push_str(header);
            if !logs.is_empty() {
                content.push('\n');
            }
        }
        content.push_str(&logs.join("\n"));
        buf.insert(0, &content);
        buf.mark_clean();
    }
    crate::window_actions::split_active(editor, crate::window::SplitAxis::Horizontal);
    if let Some(w) = editor.active_window_mut() {
        w.buffer = buf_id;
        w.cursor = crate::cursor::Cursor::default();
        w.top_line = 0;
        w.left_col = 0;
        w.selection = crate::cursor::Selection::None;
    }
}

// ── Host-side data stored inside the WASM store ──────────────────────────────

/// Everything the host-function layer needs to read/mutate during a WASM call.
/// Snapshot fields are populated before each call; mutations accumulate in
/// `pending` and are replayed onto `&mut Editor` after the call returns.
pub struct HostData {
    pub plugin_name: String,
    pub pending: Vec<PendingAction>,
    // Read-only editor snapshot
    pub active_buffer_id: Option<u32>,
    pub active_window_id: Option<u32>,
    pub cursor_cache: HashMap<u32, (usize, usize)>,
    /// Snapshot of command names already registered in the editor, used by
    /// jvim_register_command to detect duplicates without editor access.
    pub registered_cmd_names: HashSet<String>,
    pub line_count_cache: HashMap<u32, usize>,
    pub line_cache: HashMap<u32, Vec<String>>,
    pub options: HashMap<String, String>,
}

impl HostData {
    fn new(plugin_name: String) -> Self {
        Self {
            plugin_name,
            pending: Vec::new(),
            active_buffer_id: None,
            active_window_id: None,
            cursor_cache: HashMap::new(),
            line_count_cache: HashMap::new(),
            line_cache: HashMap::new(),
            options: HashMap::new(),
            registered_cmd_names: HashSet::new(),
        }
    }
}

// ── Exported function handles (all Copy) ─────────────────────────────────────

struct PluginExports {
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: TypedFunc<(i32, i32), ()>,
    jvim_init: TypedFunc<(i32, i32), i32>,
    on_event: Option<TypedFunc<(i32, i32), ()>>,
    run_command: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
    // Plugin manager exports (optional — only manager plugins export these)
    load_plugin: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
    unload_plugin: Option<TypedFunc<(i32, i32), i32>>,
}

// ── PluginInstance ────────────────────────────────────────────────────────────

pub struct PluginInstance {
    pub name: String,
    store: Store<HostData>,
    exports: PluginExports,
    pub registered_commands: Vec<String>,
}

impl PluginInstance {
    /// Populate the HostData snapshot from the current editor state.
    fn snapshot_editor(&mut self, editor: &Editor) {
        let data = self.store.data_mut();
        data.active_buffer_id = editor.active_buffer_id().map(|id| id.0);
        data.active_window_id = editor.active_window().map(|w| w.id.0);

        data.cursor_cache.clear();
        for (id, win) in &editor.windows {
            data.cursor_cache.insert(id.0, (win.cursor.row, win.cursor.col));
        }

        data.line_count_cache.clear();
        data.line_cache.clear();
        for (id, buf) in &editor.buffers {
            data.line_count_cache.insert(id.0, buf.line_count());
            let lines: Vec<String> =
                (0..buf.line_count()).map(|i| buf.line(i).to_string()).collect();
            data.line_cache.insert(id.0, lines);
        }

        data.options.clear();
        let opts = &editor.config.options;
        data.options.insert("tab_width".into(), opts.tab_width.to_string());
        data.options.insert("expandtab".into(), opts.expandtab.to_string());
        data.options.insert("number".into(), opts.number.to_string());
        data.options.insert("leader".into(), opts.leader.clone());

        data.registered_cmd_names = editor.commands.all_names().into_iter().collect();
    }

    /// Allocate `bytes` in plugin memory. Returns `(ptr, len)` on success.
    fn write_to_plugin(&mut self, bytes: &[u8]) -> Option<(i32, i32)> {
        let len = bytes.len() as i32;
        let alloc = self.exports.alloc.clone();
        let ptr = alloc.call(&mut self.store, len).ok()?;
        let memory = self.exports.memory;
        memory.write(&mut self.store, ptr as usize, bytes).ok()?;
        Some((ptr, len))
    }

    fn free_in_plugin(&mut self, ptr: i32, len: i32) {
        let dealloc = self.exports.dealloc.clone();
        let _ = dealloc.call(&mut self.store, (ptr, len));
    }

    /// Call `jvim_init` with the plugin options as JSON.
    ///
    /// Returns `Ok(manager_exts)` on success.
    /// Returns `Err((description, log_lines))` on failure — the caller is
    /// responsible for showing the error (e.g. in a scratch buffer).
    /// Registered commands are added to `self.registered_commands` internally.
    pub fn call_init(
        &mut self,
        editor: &mut Editor,
        options_json: &str,
    ) -> Result<Vec<String>, (String, Vec<String>)> {
        self.snapshot_editor(editor);
        let (ptr, len) = self
            .write_to_plugin(options_json.as_bytes())
            .ok_or_else(|| ("failed to write options to plugin memory".to_string(), vec![]))?;
        let jvim_init = self.exports.jvim_init.clone();
        let result = jvim_init.call(&mut self.store, (ptr, len));
        self.free_in_plugin(ptr, len);
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let ret = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[plugin:{}] jvim_init trapped: {e}", self.name);
                let ar = apply_pending(editor, pending, &self.name);
                return Err((format!("jvim_init trapped: {e}"), ar.log_lines));
            }
        };
        if ret != 0 {
            let ar = apply_pending(editor, pending, &self.name);
            return Err(("jvim_init returned non-zero".to_string(), ar.log_lines));
        }
        let ar = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(ar.new_commands);
        Ok(ar.new_manager_exts)
    }

    /// Call `load_plugin` to load a sub-plugin by name with its source content.
    /// Returns `true` on success.
    pub fn call_load_plugin(
        &mut self,
        editor: &mut Editor,
        name: &str,
        content: &str,
    ) -> bool {
        let load_plugin = match self.exports.load_plugin.clone() {
            Some(f) => f,
            None => {
                tracing::warn!("[plugin:{}] has no load_plugin export", self.name);
                return false;
            }
        };
        self.snapshot_editor(editor);
        let (name_ptr, name_len) = match self.write_to_plugin(name.as_bytes()) {
            Some(x) => x,
            None => return false,
        };
        let (content_ptr, content_len) = match self.write_to_plugin(content.as_bytes()) {
            Some(x) => x,
            None => {
                self.free_in_plugin(name_ptr, name_len);
                return false;
            }
        };
        let result =
            load_plugin.call(&mut self.store, (name_ptr, name_len, content_ptr, content_len));
        self.free_in_plugin(name_ptr, name_len);
        self.free_in_plugin(content_ptr, content_len);
        if let Err(e) = result {
            tracing::warn!("[plugin:{}] load_plugin trapped: {e}", self.name);
            return false;
        }
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let ar = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(ar.new_commands);
        true
    }

    /// Call `unload_plugin` to unload a previously loaded sub-plugin.
    pub fn call_unload_plugin(&mut self, editor: &mut Editor, name: &str) {
        let unload_plugin = match self.exports.unload_plugin.clone() {
            Some(f) => f,
            None => return,
        };
        self.snapshot_editor(editor);
        let (name_ptr, name_len) = match self.write_to_plugin(name.as_bytes()) {
            Some(x) => x,
            None => return,
        };
        let result = unload_plugin.call(&mut self.store, (name_ptr, name_len));
        self.free_in_plugin(name_ptr, name_len);
        if let Err(e) = result {
            tracing::warn!("[plugin:{}] unload_plugin trapped: {e}", self.name);
        }
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        apply_pending(editor, pending, &self.name);
    }

    pub fn call_on_event(&mut self, editor: &mut Editor, event_json: &str) {
        let on_event = match self.exports.on_event.clone() {
            Some(f) => f,
            None => return,
        };
        self.snapshot_editor(editor);
        let (ptr, len) = match self.write_to_plugin(event_json.as_bytes()) {
            Some(x) => x,
            None => return,
        };
        let result = on_event.call(&mut self.store, (ptr, len));
        self.free_in_plugin(ptr, len);
        if let Err(e) = result {
            tracing::warn!("[plugin:{}] on_event trapped: {e}", self.name);
        }
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let ar = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(ar.new_commands);
    }

    pub fn call_run_command(&mut self, editor: &mut Editor, cmd_name: &str, args_json: &str) {
        let run_command = match self.exports.run_command.clone() {
            Some(f) => f,
            None => {
                tracing::warn!("[plugin:{}] has no run_command export", self.name);
                return;
            }
        };
        self.snapshot_editor(editor);
        let (name_ptr, name_len) = match self.write_to_plugin(cmd_name.as_bytes()) {
            Some(x) => x,
            None => return,
        };
        let (args_ptr, args_len) = match self.write_to_plugin(args_json.as_bytes()) {
            Some(x) => x,
            None => {
                self.free_in_plugin(name_ptr, name_len);
                return;
            }
        };
        let result =
            run_command.call(&mut self.store, (name_ptr, name_len, args_ptr, args_len));
        self.free_in_plugin(name_ptr, name_len);
        self.free_in_plugin(args_ptr, args_len);
        if let Err(e) = result {
            tracing::warn!("[plugin:{}] run_command trapped: {e}", self.name);
        }
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let ar = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(ar.new_commands);
    }
}

// ── PluginExCommand — bridges an editor ex command to a WASM plugin ───────────

/// An ex command owned by a WASM plugin.  Registered in `CommandRegistry` when
/// a plugin calls `jvim_register_command(name)`.  When invoked, it forwards to
/// the plugin's `run_command` export with `args.raw` (the raw text after the
/// command name) as the argument — no JSON wrapping.
struct PluginExCommand {
    /// Static reference to the command name (leaked once; there are at most a
    /// handful of plugin commands in a session, so the leak is negligible).
    static_name: &'static str,
    /// Name of the owning plugin instance (e.g. `"mlua_wasm"`).
    plugin_name: String,
    /// The command name as passed to `jvim_register_command` (e.g. `"lua"`).
    cmd_name: String,
}

impl PluginExCommand {
    fn new(plugin_name: &str, cmd_name: &str) -> Self {
        let static_name: &'static str = Box::leak(cmd_name.to_string().into_boxed_str());
        Self {
            static_name,
            plugin_name: plugin_name.to_string(),
            cmd_name: cmd_name.to_string(),
        }
    }
}

impl ExCommand for PluginExCommand {
    fn name(&self) -> &'static str {
        self.static_name
    }

    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let mut instances = std::mem::take(&mut editor.plugins.instances);
        let result = match instances.iter_mut().find(|i| i.name == self.plugin_name) {
            Some(inst) => {
                // Pass the raw argument text directly — the plugin decides how
                // to interpret it (e.g. mlua-wasm evals it as Lua code).
                inst.call_run_command(editor, &self.cmd_name, args.raw.trim());
                Ok(())
            }
            None => Err(CommandError::Failed(format!(
                "plugin {:?} (owner of :{}) is not loaded",
                self.plugin_name, self.cmd_name
            ))),
        };
        editor.plugins.instances = instances;
        result
    }
}

// ── WASM module compilation cache ────────────────────────────────────────────

/// Try to load a pre-compiled module from disk; compile and cache on miss.
///
/// Cache files live in `~/.cache/jvim/` and are named
/// `<plugin_name>-<wasm_mtime_secs>-<wasm_size>.cwasm`.  Including the source
/// file's mtime + size in the name means a changed `.wasm` automatically
/// produces a fresh cache entry (old entries are left and must be GC'd manually,
/// but they're tiny relative to the compiled output for most plugins).
///
/// wasmtime's `deserialize_file` validates that the compiled module is
/// compatible with the running engine version, so a stale entry from an old
/// wasmtime build is detected and the module is recompiled automatically.
#[cfg(feature = "runtime-wasmtime")]
fn load_or_compile_module(
    engine: &runtime::Engine,
    name: &str,
    wasm_path: &std::path::Path,
    wasm_bytes: &[u8],
) -> anyhow::Result<Module> {
    use std::time::UNIX_EPOCH;

    // Derive a cache-busting tag from the source file's mtime + size.
    let tag = wasm_path
        .metadata()
        .ok()
        .and_then(|m| {
            let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some(format!("{}-{}", mtime, m.len()))
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Cache file: ~/.cache/jvim/<name>-<tag>.cwasm
    let cache_path = std::env::var("HOME")
        .ok()
        .map(|h| {
            std::path::PathBuf::from(h)
                .join(".cache")
                .join("jvim")
                .join(format!("{name}-{tag}.cwasm"))
        });

    // Try the cache first.
    if let Some(ref cp) = cache_path {
        if cp.exists() {
            // SAFETY: the file was written by our own `serialize()` call below,
            // so it was produced by a compatible engine.  wasmtime additionally
            // validates the embedded version/target hash and returns Err on
            // mismatch, so a stale cache from an old build is handled gracefully.
            match unsafe { Module::deserialize_file(engine, cp) } {
                Ok(m) => {
                    tracing::debug!("plugin {name:?}: loaded from compiled cache");
                    return Ok(m);
                }
                Err(e) => {
                    tracing::debug!("plugin {name:?}: cache invalid ({e}), recompiling");
                    let _ = std::fs::remove_file(cp);
                }
            }
        }
    }

    // Compile from source.
    let module = Module::new(engine, wasm_bytes)?;

    // Write the compiled module to cache (best-effort; failure is non-fatal).
    if let Some(ref cp) = cache_path {
        if let Some(parent) = cp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match module.serialize() {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(cp, &bytes) {
                    tracing::warn!("plugin {name:?}: could not write module cache: {e}");
                } else {
                    tracing::debug!("plugin {name:?}: wrote compiled cache ({} KB)", bytes.len() / 1024);
                }
            }
            Err(e) => tracing::warn!("plugin {name:?}: serialize failed: {e}"),
        }
    }

    Ok(module)
}

/// Non-wasmtime path: wasmi has no serialization support, just compile directly.
#[cfg(not(feature = "runtime-wasmtime"))]
fn load_or_compile_module(
    engine: &runtime::Engine,
    _name: &str,
    _wasm_path: &std::path::Path,
    wasm_bytes: &[u8],
) -> anyhow::Result<Module> {
    Ok(Module::new(engine, wasm_bytes)?)
}

// ── PluginManager ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct PluginManager {
    pub instances: Vec<PluginInstance>,
    /// Maps file extension (e.g. `".lua"`) to the name of the managing plugin.
    pub managers: HashMap<String, String>,
    /// Maps sub-plugin base name (e.g. `"hello"`) to its managing plugin name.
    pub managed_plugins: HashMap<String, String>,
    listener_registered: bool,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load one plugin. Tries `.wasm` first, then falls back to extension managers.
    pub fn load(&mut self, editor: &mut Editor, entry: &PluginEntry) -> Result<(), String> {
        let raw_name = entry.name().to_string();
        let options = entry.options();

        // If the name ends with a registered extension (e.g. "hello.lua"), strip it.
        let explicit_ext = self
            .managers
            .keys()
            .find(|ext| raw_name.ends_with(ext.as_str()))
            .cloned();
        let base_name = match &explicit_ext {
            Some(ext) => raw_name[..raw_name.len() - ext.len()].to_string(),
            None => raw_name.clone(),
        };

        // 1. Try in-process Lua (.lua) — takes precedence over WASM for .lua files.
        #[cfg(feature = "lua-engine")]
        if explicit_ext.is_none() || explicit_ext.as_deref() == Some(".lua") {
            let lua_path = loader::plugin_path_with_ext(&base_name, ".lua");
            if lua_path.exists() {
                let content = std::fs::read_to_string(&lua_path).map_err(|e| {
                    format!("plugin {base_name:?}: cannot read {lua_path:?}: {e}")
                })?;
                let mut lua = std::mem::take(&mut editor.lua);
                let result = lua.load(editor, &base_name, &content);
                editor.lua = lua;
                return result.map_err(|e| format!("plugin {base_name:?}: {e}"));
            }
        }

        // 2. Try .wasm (only when no explicit extension was given).
        if explicit_ext.is_none() {
            let path = loader::plugin_path(&base_name);
            if path.exists() {
                return self.load_wasm(editor, &base_name, &options, &path);
            }
        }

        // 2. Try extension managers.
        let to_try: Vec<(String, String)> = match explicit_ext {
            Some(ref ext) => self
                .managers
                .get(ext)
                .map(|m| vec![(ext.clone(), m.clone())])
                .unwrap_or_default(),
            None => self.managers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };

        for (ext, manager_name) in &to_try {
            let plugin_path = loader::plugin_path_with_ext(&base_name, ext);
            if plugin_path.exists() {
                let content = std::fs::read_to_string(&plugin_path).map_err(|e| {
                    format!("plugin {base_name:?}: cannot read {plugin_path:?}: {e}")
                })?;
                let manager_idx = self
                    .instances
                    .iter()
                    .position(|i| i.name == *manager_name)
                    .ok_or_else(|| {
                        format!(
                            "plugin {base_name:?}: manager {manager_name:?} is not loaded; \
                             ensure it appears before this plugin in config"
                        )
                    })?;
                if !self.instances[manager_idx].call_load_plugin(editor, &base_name, &content) {
                    return Err(format!(
                        "plugin {base_name:?}: manager {manager_name:?} failed to load it"
                    ));
                }
                self.managed_plugins.insert(base_name.clone(), manager_name.clone());
                return Ok(());
            }
        }

        Err(format!("plugin {base_name:?}: no .wasm found and no matching managed plugin file"))
    }

    fn load_wasm(
        &mut self,
        editor: &mut Editor,
        name: &str,
        options: &HashMap<String, String>,
        path: &std::path::Path,
    ) -> Result<(), String> {
        // Helper: show `msg` (+optional log lines) in a scratch buffer then return Err.
        // Defined as a macro so it can borrow `editor` and `name` without closure issues.
        macro_rules! fail {
            ($msg:expr) => {{
                let msg: String = $msg;
                show_plugin_error_log(editor, name, &msg, &[]);
                return Err(msg);
            }};
            ($msg:expr, $logs:expr) => {{
                let msg: String = $msg;
                show_plugin_error_log(editor, name, &msg, $logs);
                return Err(msg);
            }};
        }

        let wasm = std::fs::read(path).map_err(|e| {
            let m = format!("plugin {name:?}: cannot read {path:?}: {e}");
            show_plugin_error_log(editor, name, &m, &[]);
            m
        })?;

        let engine = runtime::make_engine();
        let module = match load_or_compile_module(&engine, name, path, &wasm) {
            Ok(m) => m,
            Err(e) => fail!(format!("plugin {name:?}: invalid WASM: {e}")),
        };

        let mut store = Store::new(&engine, HostData::new(name.to_string()));
        let mut linker: Linker<HostData> = Linker::new(&engine);

        if let Err(e) = abi::register(&mut linker) {
            fail!(format!("plugin {name:?}: ABI registration failed: {e}"));
        }
        if let Err(e) = runtime::stub_unknown_imports(&mut linker, &module) {
            fail!(format!("plugin {name:?}: import stub failed: {e}"));
        }

        let instance = match runtime::instantiate(&linker, &mut store, &module) {
            Ok(i) => i,
            Err(e) => fail!(format!("plugin {name:?}: instantiation failed: {e}")),
        };

        let memory = match instance.get_export(&mut store, "memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None => fail!(format!("plugin {name:?}: missing 'memory' export")),
        };

        let alloc: TypedFunc<i32, i32> = match instance.get_typed_func::<i32, i32>(&mut store, "alloc") {
            Ok(f) => f,
            Err(e) => fail!(format!("plugin {name:?}: missing 'alloc': {e}")),
        };

        let dealloc: TypedFunc<(i32, i32), ()> = match instance.get_typed_func::<(i32, i32), ()>(&mut store, "dealloc") {
            Ok(f) => f,
            Err(e) => fail!(format!("plugin {name:?}: missing 'dealloc': {e}")),
        };

        let jvim_init: TypedFunc<(i32, i32), i32> = match instance.get_typed_func::<(i32, i32), i32>(&mut store, "jvim_init") {
            Ok(f) => f,
            Err(e) => fail!(format!("plugin {name:?}: missing 'jvim_init': {e}")),
        };

        let on_event = instance.get_typed_func::<(i32, i32), ()>(&mut store, "on_event").ok();
        let run_command =
            instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "run_command").ok();
        let load_plugin =
            instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "load_plugin").ok();
        let unload_plugin =
            instance.get_typed_func::<(i32, i32), i32>(&mut store, "unload_plugin").ok();

        let exports = PluginExports {
            memory,
            alloc,
            dealloc,
            jvim_init,
            on_event,
            run_command,
            load_plugin,
            unload_plugin,
        };
        let mut plugin = PluginInstance {
            name: name.to_string(),
            store,
            exports,
            registered_commands: Vec::new(),
        };

        let options_json = serde_json::to_string(options).unwrap_or_else(|_| "{}".to_string());
        let manager_exts = match plugin.call_init(editor, &options_json) {
            Ok(exts) => exts,
            Err((msg, logs)) => {
                let full = format!("plugin {name:?}: {msg}");
                fail!(full, &logs);
            }
        };

        // Register any extension → manager associations declared by this plugin.
        for ext in manager_exts {
            tracing::info!("[plugin:{name}] registered as plugin manager for {ext:?}");
            self.managers.insert(ext, name.to_string());
        }

        // Register each plugin-declared command as an ex command in the global
        // registry so users can type `:cmd args` directly (not just
        // `:plugin name.cmd(args)`).  Duplicates are already blocked by
        // jvim_register_command returning -1, so this list only contains new names.
        for cmd_name in &plugin.registered_commands {
            tracing::info!("[plugin:{name}] registering ex command :{cmd_name}");
            editor.commands.register(Arc::new(PluginExCommand::new(name, cmd_name)));
        }

        self.instances.push(plugin);
        Ok(())
    }
}

// ── PluginListener ────────────────────────────────────────────────────────────

pub struct PluginListener;

impl Listener for PluginListener {
    fn on_event(&mut self, editor: &mut Editor, event: &Event<'_>) {
        let json = match event {
            Event::BufferChanged { buffer, .. } => {
                format!(r#"{{"type":"buffer_changed","buffer_id":{}}}"#, buffer.0)
            }
            Event::BufferSaved(id) => {
                format!(r#"{{"type":"buffer_saved","buffer_id":{}}}"#, id.0)
            }
            Event::BufferOpened(id) => {
                format!(r#"{{"type":"buffer_opened","buffer_id":{}}}"#, id.0)
            }
            Event::CursorMoved { window } => {
                format!(r#"{{"type":"cursor_moved","window_id":{}}}"#, window.0)
            }
            Event::ModeChanged { from, to } => {
                format!(
                    r#"{{"type":"mode_changed","from":"{}","to":"{}"}}"#,
                    mode_name(*from),
                    mode_name(*to)
                )
            }
            Event::WindowResized => r#"{"type":"window_resized"}"#.to_string(),
            Event::Quit => r#"{"type":"quit"}"#.to_string(),
        };
        let mut instances = std::mem::take(&mut editor.plugins.instances);
        for inst in &mut instances {
            inst.call_on_event(editor, &json);
        }
        editor.plugins.instances = instances;
    }
}

fn mode_name(mode: crate::mode::ModeId) -> &'static str {
    use crate::mode::ModeId::*;
    match mode {
        Normal => "normal",
        Insert => "insert",
        Visual => "visual",
        VisualLine => "vline",
        VisualBlock => "vblock",
        Command => "command",
        Search => "search",
    }
}

// ── PluginDispatch (the `:plugin` ex command) ─────────────────────────────────

pub struct PluginDispatch;

impl ExCommand for PluginDispatch {
    fn name(&self) -> &'static str {
        "plugin"
    }

    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let call = args.raw.trim();
        if call.is_empty() {
            return Err(CommandError::BadArgs(
                "usage: plugin load <name> | plugin unload <name> | plugin <name>.<fn>([args])"
                    .into(),
            ));
        }

        // `:plugin load <name>` — load a plugin at runtime.
        if let Some(name) = call.strip_prefix("load ") {
            return do_load(editor, name.trim());
        }
        // `:plugin unload <name>` — unload a running plugin.
        if let Some(name) = call.strip_prefix("unload ") {
            return do_unload(editor, name.trim());
        }
        if call == "load" || call == "unload" {
            return Err(CommandError::BadArgs(format!("usage: plugin {call} <name>")));
        }

        // `:plugin <name>.<function>([args])` — dispatch into a loaded plugin.
        let (dotted, arg_str) = if let Some(paren) = call.find('(') {
            let end = call.rfind(')').unwrap_or(call.len());
            (&call[..paren], &call[paren + 1..end])
        } else {
            (call, "")
        };

        let (plugin_name, function) = match dotted.split_once('.') {
            Some(pair) => pair,
            None => {
                return Err(CommandError::BadArgs(format!(
                    "expected <name>.<function>(), got: {call}"
                )))
            }
        };

        let args_json = format!("[{}]", arg_str.trim());
        let mut instances = std::mem::take(&mut editor.plugins.instances);

        // First: try a direct WASM plugin instance.
        let direct = instances.iter_mut().find(|i| i.name == plugin_name);
        if let Some(inst) = direct {
            inst.call_run_command(editor, function, &args_json);
            editor.plugins.instances = instances;
            return Ok(());
        }

        // Second: try routing through a plugin manager (WASM managed plugins).
        let manager_name = editor.plugins.managed_plugins.get(plugin_name).cloned();
        if let Some(mgr) = manager_name {
            let namespaced = format!("{plugin_name}.{function}");
            let result = match instances.iter_mut().find(|i| i.name == mgr) {
                Some(mgr_inst) => {
                    mgr_inst.call_run_command(editor, &namespaced, &args_json);
                    Ok(())
                }
                None => Err(CommandError::Failed(format!(
                    "plugin manager {mgr:?} for {plugin_name:?} is not loaded"
                ))),
            };
            editor.plugins.instances = instances;
            return result;
        }

        editor.plugins.instances = instances;

        // Third: try in-process Lua engine.
        #[cfg(feature = "lua-engine")]
        if editor.lua.has_plugin(plugin_name) {
            let mut lua = std::mem::take(&mut editor.lua);
            lua.run_command(editor, plugin_name, function, &args_json);
            editor.lua = lua;
            return Ok(());
        }

        Err(CommandError::Failed(format!("plugin {plugin_name:?} not loaded")))
    }

    fn complete_arg(&self, arg_idx: usize, before: &[String]) -> crate::command::ArgCompletion {
        use crate::command::ArgCompletion;
        match (arg_idx, before.first().map(|s| s.as_str())) {
            // First word: suggest the two subcommands (dispatch form has no space).
            (1, _) => ArgCompletion::Enum(&["load", "unload"]),
            // Second word after "unload": complete from currently loaded plugin names.
            (2, Some("unload")) => {
                ArgCompletion::Dynamic(|editor, partial| {
                    editor
                        .plugins
                        .instances
                        .iter()
                        .map(|i| i.name.clone())
                        .filter(|n| n.starts_with(partial))
                        .collect()
                })
            }
            _ => ArgCompletion::None,
        }
    }
}

fn do_load(editor: &mut Editor, name: &str) -> Result<(), CommandError> {
    if name.is_empty() {
        return Err(CommandError::BadArgs("usage: plugin load <name>".into()));
    }
    if editor.plugins.instances.iter().any(|i| i.name == name)
        || editor.plugins.managed_plugins.contains_key(name)
    {
        return Err(CommandError::Failed(format!("plugin {name:?} is already loaded")));
    }
    let entry = PluginEntry::Simple(name.to_string());
    let mut pm = std::mem::take(&mut editor.plugins);
    let result = pm.load(editor, &entry);
    if !pm.listener_registered && !pm.instances.is_empty() {
        editor.events.subscribe(Box::new(PluginListener));
        pm.listener_registered = true;
    }
    editor.plugins = pm;
    match result {
        Ok(()) => {
            editor.status_message = Some(format!("plugin {name:?} loaded"));
            Ok(())
        }
        Err(e) => Err(CommandError::Failed(e)),
    }
}

fn do_unload(editor: &mut Editor, name: &str) -> Result<(), CommandError> {
    if name.is_empty() {
        return Err(CommandError::BadArgs("usage: plugin unload <name>".into()));
    }
    // Direct WASM plugin?
    if let Some(idx) = editor.plugins.instances.iter().position(|i| i.name == name) {
        editor.plugins.instances.remove(idx);
        editor.status_message = Some(format!("plugin {name:?} unloaded"));
        return Ok(());
    }
    // Managed sub-plugin?
    if let Some(manager_name) = editor.plugins.managed_plugins.remove(name) {
        let mut instances = std::mem::take(&mut editor.plugins.instances);
        match instances.iter_mut().find(|i| i.name == manager_name) {
            Some(mgr) => mgr.call_unload_plugin(editor, name),
            None => {}
        }
        editor.plugins.instances = instances;
        editor.status_message = Some(format!("plugin {name:?} unloaded"));
        return Ok(());
    }
    // In-process Lua plugin?
    #[cfg(feature = "lua-engine")]
    if editor.lua.has_plugin(name) {
        editor.lua.unload(name);
        editor.status_message = Some(format!("plugin {name:?} unloaded"));
        return Ok(());
    }
    Err(CommandError::Failed(format!("plugin {name:?} is not loaded")))
}

// ── apply_config helper (called from editor.rs) ───────────────────────────────

/// Load plugins declared in config and register the event listener (once).
/// Takes `plugin_manager` out of the editor to avoid split-borrow, works on it,
/// then puts it back. Must be called after `editor.config` has been updated.
pub fn load_from_config(editor: &mut Editor) {
    let entries: Vec<PluginEntry> = editor.config.plugins.clone();
    let mut pm = std::mem::take(&mut editor.plugins);
    pm.instances.clear();
    pm.managers.clear();
    pm.managed_plugins.clear();
    for entry in &entries {
        if let Err(e) = pm.load(editor, entry) {
            tracing::warn!("{e}");
        }
    }
    if !pm.listener_registered && !pm.instances.is_empty() {
        editor.events.subscribe(Box::new(PluginListener));
        pm.listener_registered = true;
    }
    editor.plugins = pm;
}

/// Load a single plugin entry (used by the deferred-startup loader in main.rs).
pub fn load_one(editor: &mut Editor, entry: &PluginEntry) {
    let mut pm = std::mem::take(&mut editor.plugins);
    if let Err(e) = pm.load(editor, entry) {
        tracing::warn!("{e}");
        editor.status_message = Some(format!("plugin load failed: {e}"));
    }
    if !pm.listener_registered && !pm.instances.is_empty() {
        editor.events.subscribe(Box::new(PluginListener));
        pm.listener_registered = true;
    }
    editor.plugins = pm;
}
