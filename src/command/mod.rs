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

    /// Command names matching `prefix`, deduplicated and sorted.
    ///
    /// When a matching key's canonical name also starts with the prefix, the
    /// canonical name is used and aliases are merged into it (so `:vs<Tab>`
    /// yields `["vsplit"]` not `["vs","vsp","vsplit"]`). When the canonical
    /// name does NOT start with the prefix (e.g. "buffers" → "ls" for `:b`),
    /// the original key is kept so the result always starts with the prefix.
    pub fn complete_names(&self, prefix: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut names: Vec<String> = self
            .by_name
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, cmd)| {
                let canonical = cmd.name();
                if canonical.starts_with(prefix) {
                    canonical.to_string()
                } else {
                    (*key).clone()
                }
            })
            .filter(|n| seen.insert(n.clone()))
            .collect();
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
    // Normalise away a leading `:` so both `:!cmd` and `!cmd` work.
    let norm = line.trim_start_matches(':').trim_start();

    // `:!cmd` — run a shell command (no range = show output in buffer split).
    if norm.starts_with('!') {
        run_shell(editor, norm[1..].trim());
        return;
    }
    // `:%!cmd` — filter entire buffer through a shell command.
    if norm.starts_with("%!") {
        let total = editor
            .active_buffer_id()
            .and_then(|id| editor.buffers.get(&id))
            .map(|b| b.line_count())
            .unwrap_or(0);
        if total > 0 {
            editor.shell_filter_range = Some((0, total - 1));
        }
        run_shell(editor, norm[2..].trim());
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

// ── Shell execution ───────────────────────────────────────────────────────────

/// Dispatch a shell command: filter selected lines when a range is stashed,
/// otherwise run and show output.
fn run_shell(editor: &mut Editor, cmd: &str) {
    if cmd.is_empty() {
        editor.status_message = Some("usage: !<command>".into());
        return;
    }
    let filter_range = editor.shell_filter_range.take();
    match filter_range {
        Some((first, last)) => filter_lines(editor, cmd, first, last),
        None => show_shell_output(editor, cmd),
    }
}

/// Pipe lines `first..=last` of the active buffer through `cmd`, replacing
/// them with the command's stdout.
fn filter_lines(editor: &mut Editor, cmd: &str, first: usize, last: usize) {
    let Some(buf_id) = editor.active_buffer_id() else { return };

    // Collect the lines as text before mutating.
    let input: String = {
        let Some(buf) = editor.buffers.get(&buf_id) else { return };
        (first..=last).map(|i| buf.line_string(i) + "\n").collect()
    };

    let output = match run_process(cmd, Some(&input)) {
        Ok(o) => o,
        Err(e) => {
            editor.status_message = Some(format!("shell: {e}"));
            return;
        }
    };

    // Ensure the rope is built for large files before editing.
    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
        buf.materialize();
    }
    let Some(buf) = editor.buffers.get_mut(&buf_id) else { return };

    let start_char = buf.line_to_char(first);
    let end_char = if last + 1 >= buf.line_count() {
        buf.len_chars()
    } else {
        buf.line_to_char(last + 1)
    };
    // Guarantee a trailing newline so the replaced block stays a complete line.
    let replacement = if output.ends_with('\n') { output } else { output + "\n" };
    buf.replace(start_char..end_char, &replacement);

    let replaced = last - first + 1;
    if let Some(w) = editor.active_window_mut() {
        w.cursor.row = first;
        w.cursor.col = 0;
        w.selection = crate::cursor::Selection::None;
    }
    editor.status_message = Some(format!("{replaced} lines filtered"));
}

/// Run `cmd` with no stdin and show stdout in a scratch buffer split.
fn show_shell_output(editor: &mut Editor, cmd: &str) {
    let output = match run_process(cmd, None) {
        Ok(o) => o,
        Err(e) => {
            editor.status_message = Some(format!("shell: {e}"));
            return;
        }
    };
    if output.is_empty() {
        editor.status_message = Some(format!("!{cmd}: (no output)"));
        return;
    }
    let buf_id = editor.open_scratch();
    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
        buf.set_name(format!("[Shell: {cmd}]"));
        buf.insert(0, &output);
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

/// Spawn `sh -c cmd`, optionally writing `input` to stdin, and return stdout.
/// Stderr is returned as the error when the process fails with no stdout.
fn run_process(cmd: &str, input: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let stdin_cfg = if input.is_some() { Stdio::piped() } else { Stdio::null() };
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(stdin_cfg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let (Some(mut stdin), Some(text)) = (child.stdin.take(), input) {
        let _ = stdin.write_all(text.as_bytes());
        // Drop stdin to signal EOF to the child.
    }

    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    if !out.status.success() && stdout.is_empty() {
        return Err(if stderr.is_empty() { "command failed".into() } else { stderr.trim_end().into() });
    }
    Ok(stdout)
}
