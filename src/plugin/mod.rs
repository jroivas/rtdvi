pub mod abi;
pub mod config;
pub mod loader;
pub mod pending;

use std::collections::HashMap;

use wasmi::{Engine, Linker, Memory, Module, Store, TypedFunc};

use crate::command::{CommandError, ExArgs, ExCommand};
use crate::event::{Event, Listener};
use crate::Editor;

use self::config::PluginEntry;
use self::pending::apply_pending;
pub use self::pending::PendingAction;

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
    }

    /// Allocate `bytes` in plugin memory. Returns `(ptr, len)` on success.
    fn write_to_plugin(&mut self, bytes: &[u8]) -> Option<(i32, i32)> {
        let len = bytes.len() as i32;
        let alloc = self.exports.alloc;
        let ptr = alloc.call(&mut self.store, len).ok()?;
        let memory = self.exports.memory;
        memory.write(&mut self.store, ptr as usize, bytes).ok()?;
        Some((ptr, len))
    }

    fn free_in_plugin(&mut self, ptr: i32, len: i32) {
        let dealloc = self.exports.dealloc;
        let _ = dealloc.call(&mut self.store, (ptr, len));
    }

    /// Call `jvim_init` with the plugin options as JSON. Returns `true` if the
    /// plugin accepted the configuration (returned 0).
    pub fn call_init(&mut self, editor: &mut Editor, options_json: &str) -> bool {
        self.snapshot_editor(editor);
        let (ptr, len) = match self.write_to_plugin(options_json.as_bytes()) {
            Some(x) => x,
            None => return false,
        };
        let jvim_init = self.exports.jvim_init;
        let result = jvim_init.call(&mut self.store, (ptr, len));
        self.free_in_plugin(ptr, len);
        let ret = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[plugin:{}] jvim_init trapped: {e}", self.name);
                return false;
            }
        };
        if ret != 0 {
            return false;
        }
        let pending = std::mem::take(&mut self.store.data_mut().pending);
        let cmds = apply_pending(editor, pending, &self.name);
        self.registered_commands.extend(cmds);
        true
    }

    pub fn call_on_event(&mut self, editor: &mut Editor, event_json: &str) {
        let on_event = match self.exports.on_event {
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
        apply_pending(editor, pending, &self.name);
    }

    pub fn call_run_command(&mut self, editor: &mut Editor, cmd_name: &str, args_json: &str) {
        let run_command = match self.exports.run_command {
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
        apply_pending(editor, pending, &self.name);
    }
}

// ── PluginManager ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct PluginManager {
    pub instances: Vec<PluginInstance>,
    listener_registered: bool,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load one plugin from the filesystem and run its `jvim_init`. On any
    /// error the plugin is skipped with a warning; the editor continues.
    pub fn load(&mut self, editor: &mut Editor, entry: &PluginEntry) {
        let name = entry.name().to_string();
        let options = entry.options();

        let path = loader::plugin_path(&name);
        let wasm = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("[plugin:{name}] cannot read {path:?}: {e}");
                return;
            }
        };

        let engine = Engine::default();
        let module = match Module::new(&engine, &wasm) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("[plugin:{name}] invalid WASM: {e}");
                return;
            }
        };

        let mut store = Store::new(&engine, HostData::new(name.clone()));
        let mut linker: Linker<HostData> = Linker::new(&engine);
        if let Err(e) = abi::register(&mut linker) {
            tracing::warn!("[plugin:{name}] ABI registration failed: {e}");
            return;
        }

        let instance = match linker.instantiate_and_start(&mut store, &module) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("[plugin:{name}] instantiation failed: {e}");
                return;
            }
        };

        let memory = match instance
            .get_export(&store, "memory")
            .and_then(|e| e.into_memory())
        {
            Some(m) => m,
            None => {
                tracing::warn!("[plugin:{name}] missing 'memory' export");
                return;
            }
        };

        let alloc: TypedFunc<i32, i32> =
            match instance.get_typed_func::<i32, i32>(&store, "alloc") {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("[plugin:{name}] missing 'alloc': {e}");
                    return;
                }
            };

        let dealloc: TypedFunc<(i32, i32), ()> =
            match instance.get_typed_func::<(i32, i32), ()>(&store, "dealloc") {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("[plugin:{name}] missing 'dealloc': {e}");
                    return;
                }
            };

        let jvim_init: TypedFunc<(i32, i32), i32> =
            match instance.get_typed_func::<(i32, i32), i32>(&store, "jvim_init") {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("[plugin:{name}] missing 'jvim_init': {e}");
                    return;
                }
            };

        let on_event: Option<TypedFunc<(i32, i32), ()>> =
            instance.get_typed_func::<(i32, i32), ()>(&store, "on_event").ok();
        let run_command: Option<TypedFunc<(i32, i32, i32, i32), i32>> =
            instance.get_typed_func::<(i32, i32, i32, i32), i32>(&store, "run_command").ok();

        let exports = PluginExports { memory, alloc, dealloc, jvim_init, on_event, run_command };
        let mut plugin = PluginInstance {
            name: name.clone(),
            store,
            exports,
            registered_commands: Vec::new(),
        };

        let options_json =
            serde_json::to_string(&options).unwrap_or_else(|_| "{}".to_string());
        if !plugin.call_init(editor, &options_json) {
            tracing::warn!("[plugin:{name}] jvim_init returned non-zero — skipping");
            return;
        }

        self.instances.push(plugin);
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
                "usage: plugin <name>.<function>([args])".into(),
            ));
        }

        // Split off argument list: "hello.greet(arg1)" → dotted="hello.greet", arg_str="arg1"
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

        // Take instances out to avoid split-borrow conflicts.
        let mut instances = std::mem::take(&mut editor.plugins.instances);
        let found = instances.iter_mut().find(|i| i.name == plugin_name);
        let result = if let Some(inst) = found {
            inst.call_run_command(editor, function, &args_json);
            Ok(())
        } else {
            Err(CommandError::Failed(format!("plugin {plugin_name:?} not loaded")))
        };
        editor.plugins.instances = instances;
        result
    }
}

// ── apply_config helper (called from editor.rs) ───────────────────────────────

/// Load plugins declared in config and register the event listener (once).
/// Takes `plugin_manager` out of the editor to avoid split-borrow, works on it,
/// then puts it back. Must be called after `editor.config` has been updated.
pub fn load_from_config(editor: &mut Editor) {
    let entries: Vec<PluginEntry> = editor.config.plugins.clone();
    let mut pm = std::mem::take(&mut editor.plugins);
    pm.instances.clear();
    for entry in &entries {
        pm.load(editor, entry);
    }
    if !pm.listener_registered && !pm.instances.is_empty() {
        editor.events.subscribe(Box::new(PluginListener));
        pm.listener_registered = true;
    }
    editor.plugins = pm;
}
