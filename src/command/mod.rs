//! Ex commands: the `:foo args` line.

pub mod builtin;
pub mod parser;

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

use crate::Editor;

pub use parser::{ExArgs, ParsedExLine};

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("unknown command: {0}")]
    Unknown(String),
    #[error("bad arguments: {0}")]
    BadArgs(String),
    #[error("{0}")]
    Failed(String),
}

/// How Tab completion should resolve a given positional argument of a
/// command. Each command declares its own contract via
/// [`ExCommand::complete_arg`]; the completion module dispatches
/// through the registry instead of switching on names.
pub enum ArgCompletion {
    /// Nothing to complete at this position. Tab is a no-op — no
    /// spurious file listing. This is the default.
    None,
    /// Complete against the filesystem (same as `:e <Tab>`).
    Path,
    /// Fixed set of candidates. Used for sub-commands (`:config show`)
    /// and value enums (`toml` / `json`).
    Enum(&'static [&'static str]),
    /// Runtime-resolved candidates. Reserved for things like
    /// `:colorscheme <Tab>` (list installed schemes) — established
    /// here so plugin commands have somewhere natural to plug in.
    Dynamic(fn(&Editor, &str) -> Vec<String>),
}

/// Anything callable from `:`. Implementations should be cheap to construct
/// once and registered via `Arc`. Plugin commands will use the same trait.
pub trait ExCommand: Send + Sync {
    fn name(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError>;

    /// Describe how Tab should complete the `arg_idx`-th positional
    /// argument (1-based: `1` = first arg after the command name).
    /// `before` is the list of already-typed arg words, letting
    /// `:config show ` switch on what came earlier in the line.
    ///
    /// Default: no completion. Override to opt into path / enum /
    /// dynamic completion. The "no" default kills spurious file
    /// listings on zero-arg commands like `:q <Tab>`.
    fn complete_arg(&self, _arg_idx: usize, _before: &[String]) -> ArgCompletion {
        ArgCompletion::None
    }
}

#[derive(Default, Clone)]
pub struct CommandRegistry {
    by_name: HashMap<String, Arc<dyn ExCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, cmd: Arc<dyn ExCommand>) {
        let name = cmd.name().to_string();
        for alias in cmd.aliases() {
            self.by_name.insert((*alias).to_string(), cmd.clone());
        }
        self.by_name.insert(name, cmd);
    }

    pub fn lookup(&self, name: &str) -> Option<Arc<dyn ExCommand>> {
        self.by_name.get(name).cloned()
    }

    /// Every registered name (primary names plus aliases), sorted. Used by
    /// command-line Tab completion.
    pub fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.by_name.keys().cloned().collect();
        names.sort();
        names
    }
}

/// Execute an ex line that came either from the `:` prompt or from
/// `Action::Ex`. Splits the borrow of the registry from `Editor` by cloning
/// the `Arc<dyn ExCommand>` out first.
pub fn run_ex_line(editor: &mut Editor, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let parsed = match parser::parse(line) {
        Ok(p) => p,
        Err(e) => {
            editor.status_message = Some(format!("E: {e}"));
            return;
        }
    };
    let Some(cmd) = editor.commands.lookup(&parsed.name) else {
        editor.status_message = Some(format!("E492: not an editor command: {}", parsed.name));
        return;
    };
    if let Err(e) = cmd.run(editor, &parsed.args) {
        editor.status_message = Some(format!("E: {e}"));
    }
}
