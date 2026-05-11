//! The `Editor`: the aggregate that owns every long-lived piece of state.
//!
//! Everything mutable in the editor (buffers, windows, registries, mode
//! state) lives here. Subsystems take `&mut Editor` so a single mutable
//! borrow flows through all dispatch paths — registries that store
//! `Arc<dyn Trait>` clone their handles out before invoking, which is what
//! keeps the borrow checker happy.

use std::collections::HashMap;
use std::path::Path;

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

    // Registries (the extension seams).
    pub commands: CommandRegistry,
    pub keymap: KeymapRegistry,
    pub actions: ActionRegistry,
    pub events: EventBus,

    // Misc.
    pub config: Config,
    pub status_message: Option<String>,
    pub should_quit: bool,
    /// Yank/put scratch register. v1 keeps just the unnamed `"` register.
    pub unnamed_register: Register,

    next_buffer_id: u32,
    next_window_id: u32,
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
            commands: CommandRegistry::new(),
            keymap: KeymapRegistry::new(),
            actions: ActionRegistry::new(),
            events: EventBus::new(),
            config: Config::default(),
            status_message: None,
            should_quit: false,
            next_buffer_id: 0,
            next_window_id: 0,
            unnamed_register: Register::default(),
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
        crate::window_actions::register_all(&mut self.actions);
        crate::window_actions::bind_default_keys(&mut self.keymap);
        crate::visual_actions::register_all(&mut self.actions);
        crate::visual_actions::bind_default_keys(&mut self.keymap);
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
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
