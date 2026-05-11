//! The `Editor`: the aggregate that owns every long-lived piece of state.
//!
//! Everything mutable in the editor (buffers, windows, registries, mode
//! state) lives here. Subsystems take `&mut Editor` so a single mutable
//! borrow flows through all dispatch paths — registries that store
//! `Arc<dyn Trait>` clone their handles out before invoking, which is what
//! keeps the borrow checker happy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::buffer::{Buffer, BufferError, BufferId};
use crate::command::{builtin, CommandRegistry};
use crate::config::Config;
use crate::cursor::Cursor;
use crate::event::EventBus;
use crate::keymap::{ActionRegistry, Key, KeymapRegistry};
use crate::mode::command::CommandLineState;
use crate::mode::ModeId;
use crate::search::SearchState;
use crate::tab::Tab;
use crate::window::{Window, WindowId};

/// Vim-style yank-and-put scratch register. Tracks whether the last yank/
/// delete was line-wise so `p` can paste below vs after.
#[derive(Default, Clone, Debug)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

pub struct Editor {
    // Documents and views.
    pub buffers: HashMap<BufferId, Buffer>,
    pub windows: HashMap<WindowId, Window>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,

    // Mode + mode-specific state.
    pub mode: ModeId,
    pub command_line: CommandLineState,
    pub search: SearchState,
    pub pending_keys: Vec<Key>,
    /// Count typed before the current operator/motion (e.g. the `4` in `4dj`).
    pub pending_count_pre: Option<usize>,
    /// Count typed between an operator and its motion (e.g. the `3` in `d3w`).
    pub pending_count_post: Option<usize>,
    /// Set by the `r` action: the next typed character is consumed as the
    /// replacement char instead of going through the keymap. Cleared as
    /// soon as the replacement (or `<Esc>` cancel) is processed.
    pub pending_replace: bool,

    // Registries (the extension seams).
    pub commands: CommandRegistry,
    pub keymap: KeymapRegistry,
    pub actions: ActionRegistry,
    pub events: EventBus,

    // Misc.
    pub config: Config,
    pub colorscheme: crate::colorscheme::Colorscheme,
    pub status_message: Option<String>,
    pub should_quit: bool,
    /// Yank/put scratch register. v1 keeps just the unnamed `"` register.
    pub unnamed_register: Register,
    /// LSP client manager. One client per (server_name, workspace_root)
    /// pair, auto-spawned when a matching filetype is opened.
    pub lsp: crate::lsp::Manager,
    /// Most recent `gr` references result — list of `(uri, line, character)`.
    /// Used by `:LspReferences` and (future) `]r` / `[r` navigation.
    pub lsp_references: Vec<(String, u32, u32)>,
    /// Set by `I` or `A` in visual-block. The next `<Esc>` from insert mode
    /// reads it and replays the typed text into every other row of the
    /// rectangle. `None` outside a block-insert session.
    pub pending_block_insert: Option<PendingBlockInsert>,

    /// Compiled syntax engine per buffer. Built on first `syntax_for(buffer)`
    /// call (which can be expensive — reads disk + compiles regexes) and
    /// reused on every subsequent render. Invalidated when a buffer's
    /// filetype could change (`:set syntax=…`, `:e`).
    syntax_cache: RefCell<HashMap<BufferId, Arc<crate::syntax::Syntax>>>,

    next_buffer_id: u32,
    next_window_id: u32,
}

/// Records the rectangle a block-insert / block-append session needs to
/// replay across when insert mode ends.
#[derive(Debug, Clone)]
pub struct PendingBlockInsert {
    /// Rows OTHER than the one being actively typed on. May be empty.
    pub other_rows: Vec<usize>,
    /// Display column where the inserted text begins on each row.
    pub col: usize,
    /// Top row of the rectangle — the user types on this row first.
    pub start_row: usize,
    /// Display column the cursor was at when `I` / `A` fired.
    pub start_col: usize,
    /// `true` for `A` (block append): pad short lines with spaces.
    /// `false` for `I` (block insert): skip lines shorter than `col`.
    pub pad_when_short: bool,
}

impl Editor {
    pub fn new() -> Self {
        let mut editor = Self {
            buffers: HashMap::new(),
            windows: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            mode: ModeId::Normal,
            command_line: CommandLineState::default(),
            search: SearchState::default(),
            pending_keys: Vec::new(),
            pending_count_pre: None,
            pending_count_post: None,
            pending_replace: false,
            commands: CommandRegistry::new(),
            keymap: KeymapRegistry::new(),
            actions: ActionRegistry::new(),
            events: EventBus::new(),
            config: Config::default(),
            colorscheme: crate::colorscheme::defaults(),
            status_message: None,
            should_quit: false,
            next_buffer_id: 0,
            next_window_id: 0,
            unnamed_register: Register::default(),
            pending_block_insert: None,
            lsp: crate::lsp::Manager::new(),
            lsp_references: Vec::new(),
            syntax_cache: RefCell::new(HashMap::new()),
        };
        editor.register_builtins();
        editor
    }

    fn register_builtins(&mut self) {
        builtin::register_all(&mut self.commands);
        crate::motion::register_all(&mut self.actions);
        crate::motion::bind_default_keys(&mut self.keymap);
        crate::edit_actions::register_all(&mut self.actions);
        crate::edit_actions::bind_default_keys(&mut self.keymap);
        crate::delete_actions::register_all(&mut self.actions);
        crate::delete_actions::bind_default_keys(&mut self.keymap);
        crate::yank_actions::register_all(&mut self.actions);
        crate::yank_actions::bind_default_keys(&mut self.keymap);
        crate::replace_actions::register_all(&mut self.actions);
        crate::replace_actions::bind_default_keys(&mut self.keymap);
        crate::bracket_actions::register_all(&mut self.actions);
        crate::bracket_actions::bind_default_keys(&mut self.keymap);
        crate::lsp_actions::register_all(&mut self.actions);
        crate::lsp_actions::bind_default_keys(&mut self.keymap);
        crate::window_actions::register_all(&mut self.actions);
        crate::window_actions::bind_default_keys(&mut self.keymap);
        crate::visual_actions::register_all(&mut self.actions);
        crate::visual_actions::bind_default_keys(&mut self.keymap);
        crate::search_actions::register_all(&mut self.actions);
        crate::search_actions::bind_default_keys(&mut self.keymap);
        // Adding `:foo` = new struct in command/builtin.rs + register_all call.
        // Adding a new action = register here + bind in default keymap (or via TOML in M10).
    }

    pub fn new_buffer_id(&mut self) -> BufferId {
        let id = BufferId(self.next_buffer_id);
        self.next_buffer_id += 1;
        id
    }

    pub fn new_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    pub fn open_path(&mut self, path: &Path) -> Result<BufferId, BufferError> {
        let id = self.new_buffer_id();
        let buf = Buffer::from_path(id, path)?;
        self.buffers.insert(id, buf);
        crate::event::emit(self, crate::event::Event::BufferOpened(id));
        // Auto-start the configured LSP server (if any) and announce the
        // buffer to it. Failures (e.g. clangd not installed) are swallowed
        // — the editor keeps working without LSP.
        self.lsp_did_open(id);
        Ok(id)
    }

    pub fn open_scratch(&mut self) -> BufferId {
        let id = self.new_buffer_id();
        let buf = Buffer::scratch(id);
        self.buffers.insert(id, buf);
        crate::event::emit(self, crate::event::Event::BufferOpened(id));
        id
    }

    /// Initialize the editor with a single tab + single window showing `buffer`.
    pub fn focus_single(&mut self, buffer: BufferId) {
        let win_id = self.new_window_id();
        let mut win = Window::new(win_id, buffer);
        win.cursor = Cursor::default();
        self.windows.insert(win_id, win);
        self.tabs.clear();
        self.tabs.push(Tab::single(win_id));
        self.active_tab = 0;
    }

    pub fn active_window(&self) -> Option<&Window> {
        let tab = self.tabs.get(self.active_tab)?;
        self.windows.get(&tab.active)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        let win_id = self.tabs.get(self.active_tab)?.active;
        self.windows.get_mut(&win_id)
    }

    pub fn active_buffer_id(&self) -> Option<BufferId> {
        self.active_window().map(|w| w.buffer)
    }

    /// Read and reset the combined count for the next action.
    /// Vim multiplies a pre-operator count by a post-operator count, so
    /// `2d3w` deletes six words; defaults are 1.
    pub fn take_count(&mut self) -> usize {
        let pre = self.pending_count_pre.take().unwrap_or(1).max(1);
        let post = self.pending_count_post.take().unwrap_or(1).max(1);
        pre.saturating_mul(post).max(1)
    }

    pub fn clear_pending_count(&mut self) {
        self.pending_count_pre = None;
        self.pending_count_post = None;
    }

    /// Open an undo transaction on the active buffer. All subsequent edits
    /// are coalesced into a single undo entry until
    /// [`Editor::end_active_transaction`] is called.
    pub fn begin_active_transaction(&mut self) {
        if let Some(id) = self.active_buffer_id() {
            if let Some(b) = self.buffers.get_mut(&id) {
                b.begin_transaction();
            }
        }
    }

    pub fn end_active_transaction(&mut self) {
        if let Some(id) = self.active_buffer_id() {
            if let Some(b) = self.buffers.get_mut(&id) {
                b.end_transaction();
            }
        }
    }

    /// Get the syntax engine for `buffer`, building it on first call and
    /// caching the result. Reading `/usr/share/vim/vim*/syntax/<lang>.vim`
    /// from disk and compiling its keyword regexes is by far the slowest
    /// thing in the render pipeline, so this needs to stay cached.
    pub fn syntax_for(&self, buffer: BufferId) -> Arc<crate::syntax::Syntax> {
        if let Some(syn) = self.syntax_cache.borrow().get(&buffer).cloned() {
            return syn;
        }
        let buf = self.buffers.get(&buffer);
        let path = buf.and_then(|b| b.path()).map(|p| p.to_path_buf());
        let manual = buf.and_then(|b| b.syntax_override());
        let overrides = crate::syntax::FiletypeOverrides::from_map(&self.config.filetypes);
        let syn = Arc::new(crate::syntax::Syntax::for_buffer(
            path.as_deref(),
            manual,
            &overrides,
        ));
        self.syntax_cache.borrow_mut().insert(buffer, syn.clone());
        syn
    }

    /// Drop the cached syntax engine for `buffer` (or all buffers if `None`)
    /// so the next call to [`syntax_for`] rebuilds it. Call after
    /// `:set syntax=…`, `:e` to a new file, or a config reload.
    pub fn invalidate_syntax_cache(&self, buffer: Option<BufferId>) {
        let mut cache = self.syntax_cache.borrow_mut();
        match buffer {
            Some(b) => {
                cache.remove(&b);
            }
            None => cache.clear(),
        }
    }

    /// Notify the LSP layer that `buffer` has been opened: auto-starts the
    /// configured server for its filetype (if any) and sends `didOpen`.
    /// Safe to call multiple times — the server tracks open versions.
    pub fn lsp_did_open(&mut self, buffer: BufferId) {
        let filetype = self.syntax_for(buffer).filetype.to_string();
        let Some(buf) = self.buffers.get(&buffer) else {
            return;
        };
        let path = match buf.path() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let Ok(uri) = lsp_types::Url::from_file_path(&path) else {
            return;
        };
        let text = buf.rope().to_string();
        if let Some(client) = self.lsp.ensure(&filetype, &path) {
            client.did_open(uri.as_str(), &filetype, &text);
        }
    }

    /// Notify the LSP layer that `buffer` has been modified. Sends a
    /// full-text `didChange` to every running client that knows the URI.
    pub fn lsp_did_change(&mut self, buffer: BufferId) {
        let Some(buf) = self.buffers.get(&buffer) else {
            return;
        };
        let path = match buf.path() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let Ok(uri) = lsp_types::Url::from_file_path(&path) else {
            return;
        };
        let text = buf.rope().to_string();
        let filetype = self.syntax_for(buffer).filetype.to_string();
        if let Some(client) = self.lsp.find_for(&filetype, &path) {
            client.did_change(uri.as_str(), &text);
        }
    }

    pub fn lsp_poll(&mut self) {
        self.lsp.poll_all();
    }

    /// Apply a `Config`: replace `self.config` and install user keymaps.
    /// User keymaps are added on top of the built-in defaults (later
    /// `bind` calls override earlier ones).
    pub fn apply_config(&mut self, config: Config) {
        for k in &config.keymaps {
            let mode_id = match k.mode.as_str() {
                "normal" | "n" => crate::mode::ModeId::Normal,
                "insert" | "i" => crate::mode::ModeId::Insert,
                "visual" | "v" => crate::mode::ModeId::Visual,
                "vline" | "V" => crate::mode::ModeId::VisualLine,
                "vblock" | "C-v" => crate::mode::ModeId::VisualBlock,
                other => {
                    self.status_message = Some(format!("config: unknown mode {other:?}"));
                    continue;
                }
            };
            let action = crate::keymap::Action::Builtin(Box::leak(k.action.clone().into_boxed_str()));
            if let Err(e) = self.keymap.bind(mode_id, &k.keys, action) {
                self.status_message = Some(format!("config: {e}"));
            }
        }
        // LSP server configs.
        let lsp_configs: Vec<crate::lsp::LspConfig> = config
            .lsp
            .iter()
            .map(|(name, c)| crate::lsp::LspConfig {
                name: name.clone(),
                cmd: c.cmd.clone(),
                filetypes: c.filetypes.clone(),
                root_markers: c.root_markers.clone(),
            })
            .collect();
        self.lsp.apply_user_configs(lsp_configs);
        self.config = config;
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
