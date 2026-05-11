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

/// Anything callable from `:`. Implementations should be cheap to construct
/// once and registered via `Arc`. Plugin commands will use the same trait.
pub trait ExCommand: Send + Sync {
    fn name(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError>;
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
