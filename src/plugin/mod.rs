pub mod abi;
pub mod config;
pub mod loader;
pub mod pending;
pub mod runtime;
mod wasm;

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use self::runtime::Module;

use crate::command::{CommandError, ExArgs, ExCommand};
use crate::event::{Event, Listener};
use crate::Editor;

use self::config::PluginEntry;
pub use self::pending::{ApplyResult, PendingAction};
#[cfg(feature = "render-buffer")]
pub use self::pending::{RenderLineSpec, RenderSpanSpec};
pub use wasm::{HostData, PluginInstance};

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

// ── PluginExCommand — bridges an editor ex command to a WASM plugin ───────────

/// An ex command owned by a WASM plugin.  Registered in `CommandRegistry` when
/// a plugin calls `rtdvi_register_command(name)`.  When invoked, it forwards to
/// the plugin's `run_command` export with `args.raw` (the raw text after the
/// command name) as the argument — no JSON wrapping.
struct PluginExCommand {
    /// Static reference to the command name (leaked once; there are at most a
    /// handful of plugin commands in a session, so the leak is negligible).
    static_name: &'static str,
    /// Name of the owning plugin instance (e.g. `"mlua_wasm"`).
    plugin_name: String,
    /// The command name as passed to `rtdvi_register_command` (e.g. `"lua"`).
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


// ── PluginManager ─────────────────────────────────────────────────────────────

/// A WASM module being compiled on a background thread.
struct CompileJob {
    name: String,
    options: HashMap<String, String>,
    path: std::path::PathBuf,
    rx: mpsc::Receiver<Result<Module, String>>,
}

#[derive(Default)]
pub struct PluginManager {
    pub instances: Vec<PluginInstance>,
    /// Maps file extension (e.g. `".lua"`) to the name of the managing plugin.
    pub managers: HashMap<String, String>,
    /// Maps sub-plugin base name (e.g. `"hello"`) to its managing plugin name.
    pub managed_plugins: HashMap<String, String>,
    /// Maps filetype (e.g. `"c"`) to the plugin name that handles its indentation.
    pub indent_providers: HashMap<String, String>,
    listener_registered: bool,
    /// Shared engine — created once, cloned for background compile threads.
    engine: Option<runtime::Engine>,
    /// Plugin entries not yet started (populated by `load_from_config`).
    pub pending: Vec<PluginEntry>,
    /// Background WASM compilation in flight (at most one at a time).
    compiling: Option<CompileJob>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn engine(&mut self) -> runtime::Engine {
        self.engine.get_or_insert_with(runtime::make_engine).clone()
    }

    /// True while a WASM module is being compiled in the background.
    pub fn is_loading(&self) -> bool {
        self.compiling.is_some() || !self.pending.is_empty()
    }

    /// Split a configured plugin name into `(explicit_extension, base_name)`.
    /// If `raw_name` ends with a registered manager extension (e.g. `.lua`),
    /// that extension is returned and stripped from the base; otherwise the
    /// whole name is the base and the extension is `None`.
    fn split_known_extension(&self, raw_name: &str) -> (Option<String>, String) {
        let explicit_ext = self
            .managers
            .keys()
            .find(|ext| raw_name.ends_with(ext.as_str()))
            .cloned();
        let base_name = match &explicit_ext {
            Some(ext) => raw_name[..raw_name.len() - ext.len()].to_string(),
            None => raw_name.to_string(),
        };
        (explicit_ext, base_name)
    }

    /// Blocking load (used by `:plugin load` at runtime). Tries `.wasm` first,
    /// then falls back to extension managers.
    pub fn load(&mut self, editor: &mut Editor, entry: &PluginEntry) -> Result<(), String> {
        let raw_name = entry.name().to_string();
        let options = entry.options();

        // If the name ends with a registered extension (e.g. "hello.lua"), strip it.
        let (explicit_ext, base_name) = self.split_known_extension(&raw_name);

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

        let engine = self.engine();
        let module = match wasm::load_or_compile_module(&engine, name, path, &wasm) {
            Ok(m) => m,
            Err(e) => {
                let msg = e.to_string();
                let hint = if msg.contains("exceptions proposal") {
                    " (plugin uses WASM exceptions; build rtdvi with runtime-wasmtime)"
                } else {
                    ""
                };
                fail!(format!("plugin {name:?}: invalid WASM: {e}{hint}"))
            }
        };

        let (plugin, manager_exts) =
            match wasm::instantiate_and_init(&engine, editor, name, options, &module) {
                Ok(x) => x,
                Err((msg, logs)) => fail!(msg, &logs),
            };

        // Register any extension → manager associations declared by this plugin.
        for ext in manager_exts {
            tracing::info!("[plugin:{name}] registered as plugin manager for {ext:?}");
            self.managers.insert(ext, name.to_string());
        }

        // Register each plugin-declared command as an ex command in the global
        // registry so users can type `:cmd args` directly (not just
        // `:plugin name.cmd(args)`).  Duplicates are already blocked by
        // rtdvi_register_command returning -1, so this list only contains new names.
        for cmd_name in &plugin.registered_commands {
            tracing::info!("[plugin:{name}] registering ex command :{cmd_name}");
            editor.commands.register(Arc::new(PluginExCommand::new(name, cmd_name)));
        }

        // Register indent provider filetypes.
        for ft in &plugin.indent_filetypes {
            tracing::info!("[plugin:{name}] registered as indent provider for {ft:?}");
            self.indent_providers.insert(ft.clone(), name.to_string());
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
        Terminal => "terminal",
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

/// Queue plugins from config for background loading.
/// Must be called after `editor.config` has been updated.
/// Actual loading happens via `tick()` calls from the run loop.
pub fn load_from_config(editor: &mut Editor) {
    let entries: Vec<PluginEntry> = editor.config.plugins.clone();
    let pm = &mut editor.plugins;
    pm.instances.clear();
    pm.managers.clear();
    pm.managed_plugins.clear();
    pm.compiling = None;
    pm.pending = entries;
}

/// Called once per frame. Starts background compilation for the next pending
/// plugin and/or completes instantiation when a compiled module is ready.
/// Non-blocking: returns immediately if no work is ready.
pub fn tick(editor: &mut Editor) {
    // Phase 1 — check if a background compile job finished.
    let job_done = editor
        .plugins
        .compiling
        .as_ref()
        .and_then(|j| j.rx.try_recv().ok());

    if let Some(result) = job_done {
        let job = editor.plugins.compiling.take().unwrap();
        match result {
            Ok(module) => wasm::finish_wasm_load(editor, job.name, job.options, job.path, module),
            Err(e) => {
                let msg = format!("plugin {:?}: compile failed: {e}", job.name);
                tracing::warn!("{msg}");
                show_plugin_error_log(editor, &job.name, &msg, &[]);
            }
        }
        // Fall through: try to start the next pending entry immediately.
    }

    // Phase 2 — start pending entries while no compile job is running.
    // Non-WASM entries (Lua via manager) complete synchronously and we keep
    // looping; WASM entries spawn a thread and we stop (compiling is set).
    while editor.plugins.compiling.is_none() && !editor.plugins.pending.is_empty() {
        let entry = editor.plugins.pending.remove(0);
        start_entry(editor, entry);
    }

    // Register the event listener once any plugin is live.
    let pm = &mut editor.plugins;
    if !pm.listener_registered && !pm.instances.is_empty() {
        editor.events.subscribe(Box::new(PluginListener));
        pm.listener_registered = true;
    }
}

/// Route one entry: fast paths (in-process Lua, extension-manager) run
/// synchronously; WASM plugins spawn a background compilation thread.
fn start_entry(editor: &mut Editor, entry: PluginEntry) {
    let raw_name = entry.name().to_string();
    let options = entry.options();

    let (explicit_ext, base_name) = editor.plugins.split_known_extension(&raw_name);

    // 1. In-process Lua engine — fast, run synchronously.
    #[cfg(feature = "lua-engine")]
    if explicit_ext.is_none() || explicit_ext.as_deref() == Some(".lua") {
        let lua_path = loader::plugin_path_with_ext(&base_name, ".lua");
        if lua_path.exists() {
            match std::fs::read_to_string(&lua_path) {
                Ok(content) => {
                    let mut lua = std::mem::take(&mut editor.lua);
                    if let Err(e) = lua.load(editor, &base_name, &content) {
                        tracing::warn!("plugin {base_name:?}: {e}");
                        editor.status_message = Some(format!("plugin {base_name:?}: {e}"));
                    }
                    editor.lua = lua;
                }
                Err(e) => {
                    tracing::warn!("plugin {base_name:?}: cannot read {lua_path:?}: {e}");
                }
            }
            return;
        }
    }

    // 2. WASM plugin — spawn background compilation.
    if explicit_ext.is_none() {
        let path = loader::plugin_path(&base_name);
        if path.exists() {
            match std::fs::read(&path) {
                Ok(wasm_bytes) => {
                    let engine = editor.plugins.engine();
                    let name_t = base_name.clone();
                    let path_t = path.clone();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let result = wasm::load_or_compile_module(&engine, &name_t, &path_t, &wasm_bytes)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(result);
                    });
                    editor.plugins.compiling = Some(CompileJob {
                        name: base_name,
                        options,
                        path,
                        rx,
                    });
                }
                Err(e) => {
                    let msg = format!("plugin {base_name:?}: cannot read {path:?}: {e}");
                    tracing::warn!("{msg}");
                    show_plugin_error_log(editor, &base_name, &msg, &[]);
                }
            }
            return;
        }
    }

    // 3. Extension manager (e.g. hello.lua loaded via mlua-wasm) — fast.
    let to_try: Vec<(String, String)> = match explicit_ext {
        Some(ref ext) => editor
            .plugins
            .managers
            .get(ext)
            .map(|m| vec![(ext.clone(), m.clone())])
            .unwrap_or_default(),
        None => editor
            .plugins
            .managers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    };

    for (ext, manager_name) in &to_try {
        let plugin_path = loader::plugin_path_with_ext(&base_name, ext);
        if plugin_path.exists() {
            let content = match std::fs::read_to_string(&plugin_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("plugin {base_name:?}: cannot read: {e}");
                    continue;
                }
            };
            let manager_idx = editor
                .plugins
                .instances
                .iter()
                .position(|i| i.name == *manager_name);
            if let Some(idx) = manager_idx {
                let mut instances = std::mem::take(&mut editor.plugins.instances);
                let ok = instances[idx].call_load_plugin(editor, &base_name, &content);
                editor.plugins.instances = instances;
                if ok {
                    editor
                        .plugins
                        .managed_plugins
                        .insert(base_name.clone(), manager_name.clone());
                }
            } else {
                tracing::warn!(
                    "plugin {base_name:?}: manager {manager_name:?} not loaded yet; \
                     ensure it appears before this plugin in config"
                );
            }
            return;
        }
    }

    tracing::warn!("plugin {raw_name:?}: no .wasm and no matching managed plugin file");
}

/// Ask the registered indent provider plugin (if any) to compute the indent
/// for the line following `prev_row` in `buf_id`. Returns `None` if no
/// provider is registered for `filetype` or the call fails.
pub fn call_plugin_indent(
    editor: &mut Editor,
    filetype: &str,
    buf_id: crate::buffer::BufferId,
    prev_row: usize,
) -> Option<String> {
    let plugin_name = editor.plugins.indent_providers.get(filetype)?.clone();
    let idx = editor
        .plugins
        .instances
        .iter()
        .position(|p| p.name == plugin_name)?;
    // Temporarily remove the instance to satisfy the borrow checker (same
    // pattern as PluginListener::on_event).
    let mut inst = editor.plugins.instances.swap_remove(idx);
    let result = inst.call_compute_indent(editor, buf_id.0, prev_row);
    editor.plugins.instances.push(inst);
    result
}
