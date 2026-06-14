//! The WASM plugin instance: host-side data, compiled-module caching,
//! instantiation, and the rtdvi_* call wrappers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::runtime::{self, Linker, Memory, Module, Store, TypedFunc};
use super::abi;
use super::pending::{apply_pending, PendingAction};
#[cfg(feature = "render-buffer")]
use super::pending::RenderLineSpec;
use super::{show_plugin_error_log, PluginExCommand};
use crate::Editor;

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
    /// rtdvi_register_command to detect duplicates without editor access.
    pub registered_cmd_names: HashSet<String>,
    pub line_count_cache: HashMap<u32, usize>,
    pub line_cache: HashMap<u32, Vec<String>>,
    pub options: HashMap<String, String>,
    /// Name of the ex-command currently being dispatched (stamped before
    /// `run_command`). Used as the `producer` of a render buffer so following a
    /// link can re-run it.
    pub current_command: Option<String>,
    /// Scratch for the render ABI: the line under construction and the
    /// completed lines, flushed into an `OpenRenderBuffer` on `rtdvi_render_open`.
    #[cfg(feature = "render-buffer")]
    pub render_current: RenderLineSpec,
    #[cfg(feature = "render-buffer")]
    pub render_lines: Vec<RenderLineSpec>,
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
            current_command: None,
            #[cfg(feature = "render-buffer")]
            render_current: Vec::new(),
            #[cfg(feature = "render-buffer")]
            render_lines: Vec::new(),
        }
    }
}

// ── Exported function handles (all Copy) ─────────────────────────────────────

struct PluginExports {
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: TypedFunc<(i32, i32), ()>,
    rtdvi_init: TypedFunc<(i32, i32), i32>,
    on_event: Option<TypedFunc<(i32, i32), ()>>,
    run_command: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
    // Plugin manager exports (optional — only manager plugins export these)
    load_plugin: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
    unload_plugin: Option<TypedFunc<(i32, i32), i32>>,
    // Indent provider export (optional — only indent-provider plugins export this)
    compute_indent: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
}

// ── PluginInstance ────────────────────────────────────────────────────────────

pub struct PluginInstance {
    pub name: String,
    store: Store<HostData>,
    exports: PluginExports,
    pub registered_commands: Vec<String>,
    /// Filetypes this plugin handles indent for (e.g. `["c", "cpp"]`).
    pub indent_filetypes: Vec<String>,
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
            // `line_string` strips the trailing newline (and handles mmap), so
            // plugins reading `rtdvi_get_line` get the line text without a `\n`.
            // (Using `buf.line(i)` here previously leaked the terminator, which
            // doubled the lines of any render buffer built from those lines.)
            let lines: Vec<String> =
                (0..buf.line_count()).map(|i| buf.line_string(i)).collect();
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

    /// Call `rtdvi_init` with the plugin options as JSON.
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
        let rtdvi_init = self.exports.rtdvi_init.clone();
        let result = rtdvi_init.call(&mut self.store, (ptr, len));
        self.free_in_plugin(ptr, len);
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let ret = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[plugin:{}] rtdvi_init trapped: {e}", self.name);
                let ar = apply_pending(editor, pending, &self.name);
                return Err((format!("rtdvi_init trapped: {e}"), ar.log_lines));
            }
        };
        if ret != 0 {
            let ar = apply_pending(editor, pending, &self.name);
            return Err(("rtdvi_init returned non-zero".to_string(), ar.log_lines));
        }
        let ar = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(ar.new_commands);
        self.indent_filetypes.extend(ar.new_indent_filetypes);
        Ok(ar.new_manager_exts)
    }

    /// Ask the plugin to compute the indent for the line that will follow
    /// `prev_row` in `buf_id`. Returns the indent string, or `None` if the
    /// plugin has no `compute_indent` export or the call fails.
    pub fn call_compute_indent(
        &mut self,
        editor: &Editor,
        buf_id: u32,
        prev_row: usize,
    ) -> Option<String> {
        let compute_fn = self.exports.compute_indent.clone()?;
        self.snapshot_editor(editor);

        // Allocate a result buffer inside the plugin.
        const MAX: i32 = 256;
        let alloc = self.exports.alloc.clone();
        let result_ptr = alloc.call(&mut self.store, MAX).ok()?;
        if result_ptr == 0 {
            return None;
        }

        let n = compute_fn
            .call(&mut self.store, (buf_id as i32, prev_row as i32, result_ptr, MAX))
            .ok()?;

        let dealloc = self.exports.dealloc.clone();
        if n <= 0 || n > MAX {
            let _ = dealloc.call(&mut self.store, (result_ptr, MAX));
            return if n == 0 { Some(String::new()) } else { None };
        }

        let mem = self.exports.memory;
        let mut buf = vec![0u8; n as usize];
        let ok = mem.read(&self.store, result_ptr as usize, &mut buf).is_ok();
        let _ = dealloc.call(&mut self.store, (result_ptr, MAX));

        // Discard any pending actions — indent computation is read-only.
        self.store.data_mut().pending.clear();

        ok.then(|| String::from_utf8_lossy(&buf).into_owned())
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
        self.store.data_mut().current_command = Some(cmd_name.to_string());
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


// ── WASM module compilation cache ────────────────────────────────────────────

/// Try to load a pre-compiled module from disk; compile and cache on miss.
///
/// Cache files live in `~/.cache/rtdvi/` and are named
/// `<plugin_name>-<wasm_mtime_secs>-<wasm_size>.cwasm`.  Including the source
/// file's mtime + size in the name means a changed `.wasm` automatically
/// produces a fresh cache entry (old entries are left and must be GC'd manually,
/// but they're tiny relative to the compiled output for most plugins).
///
/// wasmtime's `deserialize_file` validates that the compiled module is
/// compatible with the running engine version, so a stale entry from an old
/// wasmtime build is detected and the module is recompiled automatically.
#[cfg(feature = "runtime-wasmtime")]
pub(super) fn load_or_compile_module(
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

    // Cache file: ~/.cache/rtdvi/<name>-<tag>.cwasm
    let cache_path = std::env::var("HOME")
        .ok()
        .map(|h| {
            std::path::PathBuf::from(h)
                .join(".cache")
                .join("rtdvi")
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
pub(super) fn load_or_compile_module(
    engine: &runtime::Engine,
    _name: &str,
    _wasm_path: &std::path::Path,
    wasm_bytes: &[u8],
) -> anyhow::Result<Module> {
    Ok(Module::new(engine, wasm_bytes)?)
}

pub(super) fn instantiate_and_init(
    engine: &runtime::Engine,
    editor: &mut Editor,
    name: &str,
    options: &HashMap<String, String>,
    module: &Module,
) -> Result<(PluginInstance, Vec<String>), (String, Vec<String>)> {
    let err = |msg: String| (msg, Vec::new());

    let mut store = Store::new(engine, HostData::new(name.to_string()));
    let mut linker: Linker<HostData> = Linker::new(engine);

    abi::register(&mut linker)
        .map_err(|e| err(format!("plugin {name:?}: ABI registration failed: {e}")))?;
    runtime::stub_unknown_imports(&mut linker, module)
        .map_err(|e| err(format!("plugin {name:?}: import stub failed: {e}")))?;

    let instance = runtime::instantiate(&linker, &mut store, module)
        .map_err(|e| err(format!("plugin {name:?}: instantiation failed: {e}")))?;

    let memory = instance
        .get_export(&mut store, "memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| err(format!("plugin {name:?}: missing 'memory' export")))?;

    macro_rules! get_func {
        ($fname:expr, $sig:ty) => {
            instance
                .get_typed_func::<$sig, _>(&mut store, $fname)
                .map_err(|e| err(format!("plugin {name:?}: missing {:?}: {e}", $fname)))?
        };
    }

    let alloc: TypedFunc<i32, i32> = get_func!("alloc", i32);
    let dealloc: TypedFunc<(i32, i32), ()> = get_func!("dealloc", (i32, i32));
    let rtdvi_init: TypedFunc<(i32, i32), i32> = get_func!("rtdvi_init", (i32, i32));

    let on_event =
        instance.get_typed_func::<(i32, i32), ()>(&mut store, "on_event").ok();
    let run_command =
        instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "run_command").ok();
    let load_plugin =
        instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "load_plugin").ok();
    let unload_plugin =
        instance.get_typed_func::<(i32, i32), i32>(&mut store, "unload_plugin").ok();
    let compute_indent =
        instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "compute_indent").ok();

    let exports = PluginExports {
        memory,
        alloc,
        dealloc,
        rtdvi_init,
        on_event,
        run_command,
        load_plugin,
        unload_plugin,
        compute_indent,
    };

    let mut plugin = PluginInstance {
        name: name.to_string(),
        store,
        exports,
        registered_commands: Vec::new(),
        indent_filetypes: Vec::new(),
    };

    let options_json = serde_json::to_string(options).unwrap_or_else(|_| "{}".to_string());
    let manager_exts = plugin
        .call_init(editor, &options_json)
        .map_err(|(msg, logs)| (format!("plugin {name:?}: {msg}"), logs))?;

    Ok((plugin, manager_exts))
}

pub(super) fn finish_wasm_load(
    editor: &mut Editor,
    name: String,
    options: HashMap<String, String>,
    path: std::path::PathBuf,
    module: Module,
) {
    let engine = editor.plugins.engine();
    let (plugin, manager_exts) =
        match instantiate_and_init(&engine, editor, &name, &options, &module) {
            Ok(x) => x,
            Err((msg, logs)) => {
                show_plugin_error_log(editor, &name, &msg, &logs);
                return;
            }
        };

    for ext in manager_exts {
        tracing::info!("[plugin:{name}] registered as plugin manager for {ext:?}");
        editor.plugins.managers.insert(ext, name.clone());
    }
    for cmd_name in &plugin.registered_commands {
        tracing::info!("[plugin:{name}] registering ex command :{cmd_name}");
        editor.commands.register(Arc::new(PluginExCommand::new(&name, cmd_name)));
    }
    for ft in &plugin.indent_filetypes {
        tracing::info!("[plugin:{name}] registered as indent provider for {ft:?}");
        editor.plugins.indent_providers.insert(ft.clone(), name.clone());
    }

    let _ = path; // used only for error context, kept for symmetry
    editor.plugins.instances.push(plugin);
}

