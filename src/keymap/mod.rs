//! Per-mode key bindings via a trie of [`Key`] sequences.
//!
//! Bindings map to named [`Action`]s rather than closures so they round-trip
//! through TOML config and a future plugin layer.

pub mod keys;
pub mod trie;

use std::collections::HashMap;
use std::sync::Arc;

use crate::mode::ModeId;
use crate::Editor;

pub use keys::{Key, KeyCode, KeyMods};
pub use trie::{KeyTrie, KeyTrieNode, Resolve};

/// What to do when a key sequence resolves.
#[derive(Clone, Debug)]
pub enum Action {
    /// Look up a registered handler by name.
    Builtin(&'static str),
    /// Run as if typed on the ex command line (no leading `:`).
    Ex(String),
    /// Apply each action in order.
    Sequence(Vec<Action>),
}

/// Function-like handler invoked from an `Action::Builtin`.
pub type ActionFn = Arc<dyn Fn(&mut Editor) + Send + Sync>;

#[derive(Default, Clone)]
pub struct ActionRegistry {
    inner: HashMap<&'static str, ActionFn>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: &'static str, f: ActionFn) {
        self.inner.insert(name, f);
    }

    pub fn lookup(&self, name: &str) -> Option<ActionFn> {
        self.inner.get(name).cloned()
    }
}

#[derive(Default)]
pub struct KeymapRegistry {
    per_mode: HashMap<ModeId, KeyTrie>,
}

impl KeymapRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, mode: ModeId, seq: &str, action: Action) -> Result<(), String> {
        let keys = keys::parse_sequence(seq)?;
        if keys.is_empty() {
            return Err(format!("empty key sequence: {seq:?}"));
        }
        let trie = self.per_mode.entry(mode).or_default();
        trie.insert(&keys, action);
        Ok(())
    }

    pub fn resolve(&self, mode: ModeId, pending: &[Key]) -> Resolve {
        match self.per_mode.get(&mode) {
            Some(t) => t.resolve(pending),
            None => Resolve::None,
        }
    }
}

/// Dispatch an [`Action`] against the editor. Free function so it can be
/// invoked from inside mode handlers without re-borrowing the registry.
pub fn dispatch_action(editor: &mut Editor, action: &Action) {
    match action {
        Action::Builtin(name) => {
            if let Some(f) = editor.actions.lookup(name) {
                f(editor);
            } else {
                editor.status_message = Some(format!("E: no such action: {name}"));
            }
        }
        Action::Ex(line) => {
            crate::command::run_ex_line(editor, line);
        }
        Action::Sequence(actions) => {
            for a in actions {
                dispatch_action(editor, a);
            }
        }
    }
}
