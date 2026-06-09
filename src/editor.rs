//! The `Editor`: the aggregate that owns every long-lived piece of state.
//!
//! Everything mutable in the editor (buffers, windows, registries, mode
//! state) lives here. Subsystems take `&mut Editor` so a single mutable
//! borrow flows through all dispatch paths — registries that store
//! `Arc<dyn Trait>` clone their handles out before invoking, which is what
//! keeps the borrow checker happy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer::{Buffer, BufferError, BufferId};
use crate::command::{builtin, CommandRegistry};
use crate::config::Config;
use crate::cursor::Cursor;
use crate::event::EventBus;
use crate::keymap::{ActionRegistry, Key, KeymapRegistry};
use crate::history::CommandHistory;
use crate::mode::command::CommandLineState;
use crate::mode::ModeId;
use crate::search::SearchState;
use crate::tab::Tab;
use crate::window::{Window, WindowId};

/// Expand a leading `~` / `~/` to the user's home directory (`$HOME`). Anything
/// else is returned unchanged. Used by path-taking ex commands (`:e`, `:w`,
/// `:tabnew`, …) so `~/foo` behaves like the shell and Tab completion.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Vim-style yank-and-put scratch register. Tracks whether the last yank/
/// delete was line-wise so `p` can paste below vs after.
#[derive(Default, Clone, Debug)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

/// Active state of an `:ff` interactive session: the matches resolved
/// Active location picker: shown as a popup in normal mode, navigation with
/// j/k or arrows, Enter to jump, Esc/q/Ctrl-C to dismiss.
#[derive(Debug, Clone)]
pub struct LocationPicker {
    /// Raw `(uri, line, character)` items.
    pub locations: Vec<(String, u32, u32)>,
    /// One display label per item (`filename:line`), shown in the popup.
    pub labels: Vec<String>,
    /// Index of the currently highlighted item.
    pub selected: usize,
}

impl LocationPicker {
    pub fn new(editor: &Editor, locations: Vec<(String, u32, u32)>) -> Self {
        // Cache file contents read from disk so multiple hits in one file are
        // read once. Open buffers are consulted first (and not cached here).
        let mut disk_cache: HashMap<std::path::PathBuf, Option<Vec<String>>> = HashMap::new();
        let labels = locations
            .iter()
            .map(|(uri, line, _col)| {
                let path = lsp_types::Url::parse(uri)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .unwrap_or_default();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| uri.clone());
                match line_snippet(editor, &path, *line as usize, &mut disk_cache) {
                    Some(snippet) => format!("{name}:{}: {snippet}", line + 1),
                    None => format!("{name}:{}", line + 1),
                }
            })
            .collect();
        Self { locations, labels, selected: 0 }
    }
}

/// The text at `line` (0-based) of `path`, for the location-picker label.
/// Prefers a matching open buffer (so unsaved edits show), falling back to a
/// cached read from disk. Returns `None` when the line can't be resolved.
fn line_snippet(
    editor: &Editor,
    path: &Path,
    line: usize,
    disk_cache: &mut HashMap<std::path::PathBuf, Option<Vec<String>>>,
) -> Option<String> {
    if let Some(buf) = editor.buffers.values().find(|b| b.path() == Some(path)) {
        if line < buf.line_count() {
            return clean_snippet(&buf.line_string(line));
        }
    }
    let lines = disk_cache.entry(path.to_path_buf()).or_insert_with(|| {
        std::fs::read_to_string(path)
            .ok()
            .map(|c| c.lines().map(str::to_string).collect())
    });
    clean_snippet(lines.as_ref()?.get(line)?)
}

/// Trim indentation/trailing space and clamp width so a long line doesn't blow
/// out the popup. Returns `None` for blank lines (nothing useful to show).
fn clean_snippet(raw: &str) -> Option<String> {
    const MAX: usize = 80;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > MAX {
        let cut: String = trimmed.chars().take(MAX).collect();
        Some(format!("{cut}…"))
    } else {
        Some(trimmed.to_string())
    }
}

/// from the current query and the highlighted selection index.
#[derive(Debug, Default, Clone)]
pub struct FzfSession {
    pub query: String,
    pub matches: Vec<String>,
    pub selected: usize,
    /// True once the user has typed the bang variant (`:ff!`). Drives
    /// the index-rebuild transition so we only re-walk the workspace
    /// when the bang is first added, not on every subsequent keystroke.
    pub bang: bool,
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
    /// Filesystem path the active config was loaded from, if any.
    /// `None` when running on defaults (no config file found, or
    /// `:config load` cleared it). Drives `:config show` / `:config path`
    /// and is updated by `:config load` / `:config convert`.
    pub config_path: Option<std::path::PathBuf>,
    pub colorscheme: crate::colorscheme::Colorscheme,
    pub status_message: Option<String>,
    pub should_quit: bool,
    /// Default destination/source register — the unnamed `""`. Yanks
    /// and deletes always update it (even when a `"<letter>` prefix
    /// also routed them elsewhere), so plain `p` pastes the most
    /// recent thing regardless of which named slot it landed in.
    pub unnamed_register: Register,
    /// Named registers (`a`..`z`) used via `"<letter>` prefix.
    pub named_registers: std::collections::HashMap<char, Register>,
    /// Pending `"<letter>` selection from the register-prefix
    /// machinery. Set by [`crate::registers::try_consume_key`]; read
    /// (and cleared) by yank/delete/paste sites through
    /// [`crate::registers::take_pending`].
    pub registers: crate::registers::Registers,
    /// Keyboard-macro state: `q{reg}`…`q` records keystrokes into a named
    /// register as text, `@{reg}` feeds them back. See [`crate::macros`].
    pub macros: crate::macros::MacroState,
    /// LSP client manager. One client per (server_name, workspace_root)
    /// pair, auto-spawned when a matching filetype is opened.
    pub lsp: crate::lsp::Manager,
    /// Active location picker (gr / gd when multiple results). Shown as a
    /// popup in normal mode; cleared on accept or cancel.
    pub lsp_picker: Option<LocationPicker>,
    /// Vim-style jumplist driving `<C-o>` / `<C-i>`. Jump actions
    /// (`gd`, `gg`, `*`, `/`, `]]`, …) append the *from* position here.
    pub jumplist: crate::jumplist::Jumplist,
    /// Cached file index for `:ff` (the fuzzy finder). Built lazily on
    /// first use.
    pub fzf_index: Option<crate::fzf::Index>,
    /// Persistent text highlights driven by `:highlight` / `:nohighlight`
    /// and `<leader>m`. Painted on every render.
    pub highlights: crate::highlights::Highlights,
    /// Live state of an in-progress `:ff` interactive session, or
    /// `None` when the user isn't currently typing an `:ff` query.
    pub fzf_state: Option<FzfSession>,
    pub history: CommandHistory,
    /// Persistent `/` and `?` search history, browsed with Up/Down at the
    /// search prompt. Mirrors [`history`](Self::history) but for searches.
    pub search_history: CommandHistory,
    /// Set by `I` or `A` in visual-block. The next `<Esc>` from insert mode
    /// reads it and replays the typed text into every other row of the
    /// rectangle. `None` outside a block-insert session.
    pub pending_block_insert: Option<PendingBlockInsert>,
    /// Line range stashed by `!` in visual/normal mode so the subsequent
    /// `:!cmd` Enter knows which lines to pipe through the shell.
    /// Cleared by Esc in command mode or after the shell command runs.
    pub shell_filter_range: Option<(usize, usize)>,
    /// Inclusive line range of the most recent visual selection (vim's `'<`/
    /// `'>` marks), stashed when `:` is pressed in a visual mode so a ranged
    /// ex command like `:'<,'>s/…/…/` knows which lines it covers.
    pub last_visual_range: Option<(usize, usize)>,

    /// `(width, height)` of the window region at the last render. Used by
    /// `:resize` / `:vertical resize` to convert a row/column delta into a
    /// split-ratio change.
    pub last_window_area: (u16, u16),

    /// Embedded terminals (`:term`), keyed by the scratch buffer that backs
    /// each terminal window. A window whose buffer id is in this map renders
    /// the terminal grid instead of buffer text and forwards keys to the job.
    pub terminals: HashMap<BufferId, crate::terminal::Terminal>,
    /// Set while a `<C-w>` window-command prefix is in flight in Terminal
    /// mode (so the next key is a window command, not sent to the job).
    pub terminal_window_cmd: bool,

    /// Loaded WASM plugins. Only present when built with the plugin system.
    #[cfg(feature = "plugins")]
    pub plugins: crate::plugin::PluginManager,
    /// In-process Lua plugin engine (requires `lua-engine` + a runtime).
    #[cfg(all(feature = "lua-engine", feature = "plugins"))]
    pub lua: crate::lua::LuaEngine,

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
            config_path: None,
            colorscheme: crate::colorscheme::defaults(),
            status_message: None,
            should_quit: false,
            next_buffer_id: 0,
            next_window_id: 0,
            unnamed_register: Register::default(),
            named_registers: std::collections::HashMap::new(),
            registers: crate::registers::Registers::new(),
            macros: crate::macros::MacroState::default(),
            pending_block_insert: None,
            shell_filter_range: None,
            last_visual_range: None,
            last_window_area: (0, 0),
            terminals: HashMap::new(),
            terminal_window_cmd: false,
            lsp: crate::lsp::Manager::new(),
            lsp_picker: None,
            jumplist: crate::jumplist::Jumplist::new(),
            fzf_index: None,
            fzf_state: None,
            history: CommandHistory::new(crate::history::history_path()),
            search_history: CommandHistory::new(crate::history::search_history_path()),
            highlights: crate::highlights::Highlights::new(),
            syntax_cache: RefCell::new(HashMap::new()),
            #[cfg(feature = "plugins")]
            plugins: crate::plugin::PluginManager::new(),
            #[cfg(all(feature = "lua-engine", feature = "plugins"))]
            lua: crate::lua::LuaEngine::new(),
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
        crate::jump_actions::register_all(&mut self.actions);
        crate::jump_actions::bind_default_keys(&mut self.keymap);
        crate::highlight_actions::register_all(&mut self.actions);
        crate::highlight_actions::bind_default_keys(&mut self.keymap);
        crate::indent_actions::register_all(&mut self.actions);
        crate::indent_actions::bind_default_keys(&mut self.keymap);
        crate::format_actions::register_all(&mut self.actions);
        crate::format_actions::bind_default_keys(&mut self.keymap);
        crate::window_actions::register_all(&mut self.actions);
        crate::window_actions::bind_default_keys(&mut self.keymap);
        crate::shell_actions::register_all(&mut self.actions);
        crate::shell_actions::bind_default_keys(&mut self.keymap);
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
        // Expand a leading `~` (so `:e ~/foo` works like the shell / Tab
        // completion), then store an absolute path so that Url::from_file_path
        // (used by LSP and gd) succeeds — it requires an absolute path and
        // returns Err(()) silently for relative ones.
        let tilde = path.to_str().map(expand_tilde);
        let path = tilde.as_deref().unwrap_or(path);
        let path_abs;
        let path = if path.is_absolute() {
            path
        } else {
            path_abs = path.canonicalize().unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            });
            path_abs.as_path()
        };
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
        // Spawn every server claiming this filetype (skipping missing ones),
        // then announce the buffer to each.
        self.lsp.ensure_all(&filetype, &path);
        for client in self.lsp.clients_for(&filetype, &path) {
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
        for client in self.lsp.clients_for(&filetype, &path) {
            client.did_change(uri.as_str(), &text);
        }
    }

    pub fn lsp_did_save(&mut self, buffer: BufferId) {
        let Some(buf) = self.buffers.get(&buffer) else { return };
        let path = match buf.path() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let Ok(uri) = lsp_types::Url::from_file_path(&path) else { return };
        let filetype = self.syntax_for(buffer).filetype.to_string();
        for client in self.lsp.clients_for(&filetype, &path) {
            client.did_save(uri.as_str());
        }
    }

    pub fn lsp_poll(&mut self) {
        self.lsp.poll_all();
    }

    /// Capture the current cursor position as a `JumpEntry`, or `None`
    /// if there's no active window.
    pub fn current_jump_entry(&self) -> Option<crate::jumplist::JumpEntry> {
        let w = self.active_window()?;
        Some(crate::jumplist::JumpEntry {
            buffer: w.buffer,
            row: w.cursor.row,
            col: w.cursor.col,
        })
    }

    /// Record the active cursor position into the jumplist. Call this
    /// IMMEDIATELY BEFORE moving the cursor in a "jump" — i.e. a motion
    /// that should be reversible with `<C-o>`.
    pub fn jumplist_record_here(&mut self) {
        if let Some(entry) = self.current_jump_entry() {
            self.jumplist.record(entry);
        }
    }

    /// Restore a `JumpEntry`: switch the active window to its buffer
    /// (if different), then move the cursor and centre it in the
    /// viewport so a `<C-o>` from far away doesn't drop you at the
    /// very bottom row.
    pub fn jumplist_goto(&mut self, entry: crate::jumplist::JumpEntry) {
        let Some(win_id) = self.tabs.get(self.active_tab).map(|t| t.active) else {
            return;
        };
        if self.buffers.contains_key(&entry.buffer) {
            if let Some(w) = self.windows.get_mut(&win_id) {
                w.buffer = entry.buffer;
                w.cursor.row = entry.row;
                w.cursor.col = entry.col;
                w.cursor.sticky_col = entry.col;
                w.left_col = 0;
                w.center_on_cursor();
            }
        }
    }

    /// Apply a `Config`: replace `self.config` and install user keymaps.
    /// User keymaps are added on top of the built-in defaults (later
    /// `bind` calls override earlier ones).
    pub fn apply_config(&mut self, config: Config) {
        let leader = config.options.leader.clone();
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
            let action = if k.action.starts_with(':') {
                crate::keymap::Action::Ex(k.action[1..].trim().to_string())
            } else if k.action.starts_with("plugin.") {
                // "plugin.name.fn()" → ex "plugin name.fn()"
                crate::keymap::Action::Ex(format!("plugin {}", &k.action["plugin.".len()..]))
            } else {
                crate::keymap::Action::Builtin(Box::leak(k.action.clone().into_boxed_str()))
            };
            // Expand `<leader>` to the configured leader text so the same
            // config works on whatever leader the user picked.
            let keys = crate::keymap::expand_leader(&k.keys, &leader);
            if let Err(e) = self.keymap.bind(mode_id, &keys, action) {
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
                init_options: c.init_options.clone(),
            })
            .collect();
        self.lsp.apply_user_configs(lsp_configs);
        self.config = config;
        #[cfg(feature = "plugins")]
        crate::plugin::load_from_config(self);
    }

    /// True when the active window backs an embedded terminal.
    pub fn active_is_terminal(&self) -> bool {
        self.active_buffer_id()
            .map(|b| self.terminals.contains_key(&b))
            .unwrap_or(false)
    }

    /// True while at least one terminal job is still running — used to drive
    /// a faster render cadence so output appears promptly.
    pub fn has_live_terminal(&self) -> bool {
        self.terminals.values().any(|t| !t.is_exited())
    }

    /// `:term` — open a new horizontal split running `command` (or the
    /// user's shell when empty) as an embedded terminal, and focus it.
    pub fn open_terminal(&mut self, command: &str) {
        // Initial size is a guess; the first render resizes the PTY to the
        // exact content rectangle the split ends up with.
        let (cols, rows) = match self.active_window() {
            Some(w) => (w.viewport_w.max(20), (w.viewport_h / 2).max(5)),
            None => (80, 24),
        };
        let cwd = self
            .active_buffer_id()
            .and_then(|id| self.buffers.get(&id))
            .and_then(|b| b.path())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        let term = match crate::terminal::Terminal::spawn(cols, rows, command, cwd.as_deref()) {
            Ok(t) => t,
            Err(e) => {
                self.status_message = Some(format!("term: {e}"));
                return;
            }
        };
        let buf_id = self.open_scratch();
        if let Some(buf) = self.buffers.get_mut(&buf_id) {
            buf.set_name(format!("!{}", term.command));
            buf.mark_clean();
        }
        crate::window_actions::split_active(self, crate::window::SplitAxis::Horizontal);
        if let Some(w) = self.active_window_mut() {
            w.buffer = buf_id;
            w.cursor = Cursor::default();
            w.top_line = 0;
            w.left_col = 0;
            w.selection = crate::cursor::Selection::None;
        }
        self.terminals.insert(buf_id, term);
        self.terminal_window_cmd = false;
        crate::mode::switch_mode(self, crate::mode::ModeId::Terminal);
    }

    /// Reload the active buffer from disk (`:e` / `:e!`). Returns the file's
    /// display name on success. `force` (the `!`) discards unsaved changes;
    /// without it a modified buffer is refused. Resyncs the syntax cache and
    /// LSP, and clamps the cursor of every window viewing the buffer into the
    /// reloaded (possibly shorter) content.
    pub fn reload_active_buffer(&mut self, force: bool) -> Result<String, String> {
        let Some(buf_id) = self.active_buffer_id() else {
            return Err("no active buffer".into());
        };
        match self.buffers.get(&buf_id) {
            Some(b) if b.path().is_none() => return Err("E32: No file name".into()),
            Some(b) if b.is_dirty() && !force => {
                return Err("E37: No write since last change (add ! to override)".into());
            }
            Some(_) => {}
            None => return Err("no active buffer".into()),
        }

        let name = match self.buffers.get_mut(&buf_id) {
            Some(b) => {
                b.reload().map_err(|e| e.to_string())?;
                b.display_name()
            }
            None => return Err("no active buffer".into()),
        };
        self.invalidate_syntax_cache(Some(buf_id));

        // Clamp every window showing this buffer into the new content.
        let tw = self.config.options.tab_width;
        let win_ids: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer == buf_id)
            .map(|(id, _)| *id)
            .collect();
        for wid in win_ids {
            let cur = self.windows.get(&wid).map(|w| (w.cursor.row, w.cursor.col));
            let Some((cur_row, cur_col)) = cur else { continue };
            let Some(b) = self.buffers.get(&buf_id) else { continue };
            let last_row = b.line_count().saturating_sub(1);
            let row = cur_row.min(last_row);
            let lw = crate::text::width::line_display_width(&b.line_string(row), tw);
            let col = cur_col.min(lw.saturating_sub(1));
            if let Some(w) = self.windows.get_mut(&wid) {
                w.cursor.row = row;
                w.cursor.col = col;
                w.cursor.sticky_col = col;
                if w.top_line > row {
                    w.top_line = row;
                }
            }
        }

        self.lsp_did_change(buf_id);
        Ok(name)
    }

    /// Close any terminals whose job has exited, removing their windows and
    /// buffers. Returns `true` if anything was reaped, so the caller can
    /// re-sync the editor mode for whatever window is now active.
    pub fn reap_terminals(&mut self) -> bool {
        let dead: Vec<BufferId> = self
            .terminals
            .iter()
            .filter(|(_, t)| t.is_exited())
            .map(|(id, _)| *id)
            .collect();
        if dead.is_empty() {
            return false;
        }
        for buf_id in dead {
            self.close_terminal_buffer(buf_id);
        }
        true
    }

    /// Tear down a terminal: kill its job (via `Terminal`'s `Drop`), close
    /// every window showing it, and drop the backing scratch buffer.
    pub fn close_terminal_buffer(&mut self, buf_id: BufferId) {
        self.terminals.remove(&buf_id);
        let win_ids: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer == buf_id)
            .map(|(id, _)| *id)
            .collect();
        for wid in win_ids {
            crate::window_actions::remove_window(self, wid);
        }
        self.buffers.remove(&buf_id);
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::expand_tilde;
    use std::path::PathBuf;

    #[test]
    fn expand_tilde_resolves_home() {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            assert_eq!(expand_tilde("~"), home);
            assert_eq!(expand_tilde("~/pkg.txt"), home.join("pkg.txt"));
            assert_eq!(expand_tilde("~/a/b"), home.join("a/b"));
        }
    }

    #[test]
    fn expand_tilde_passes_other_paths_through() {
        assert_eq!(expand_tilde("/abs/x"), PathBuf::from("/abs/x"));
        assert_eq!(expand_tilde("rel/x"), PathBuf::from("rel/x"));
        // `~user` (other users) is not expanded.
        assert_eq!(expand_tilde("~user/x"), PathBuf::from("~user/x"));
    }
}
