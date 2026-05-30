//! Command-line history: persistence and in-session browsing.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_HISTORY: usize = 10_000;

pub struct CommandHistory {
    /// All known entries, oldest first.
    pub entries: Vec<String>,
    /// Index into `entries` while the user is pressing Up/Down.
    /// `None` means we are at the live input (not browsing).
    pub browse_index: Option<usize>,
    /// The text the user had typed before the first Up press; restored when
    /// they press Down past the newest entry.
    pub saved_input: String,
    /// Prefix filter captured on the first Up press. Only entries that
    /// `starts_with` this string are visited during browsing.
    pub prefix: String,
    /// Absolute path to the history file on disk.
    pub file_path: PathBuf,
}

impl CommandHistory {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            entries: Vec::new(),
            browse_index: None,
            saved_input: String::new(),
            prefix: String::new(),
            file_path,
        }
    }

    /// Add a command to history. Skips empty strings and consecutive
    /// duplicates, then appends to the on-disk file.
    pub fn add(&mut self, cmd: String) {
        if cmd.is_empty() {
            return;
        }
        if self.entries.last().map(|s| s.as_str()) == Some(cmd.as_str()) {
            return;
        }
        self.entries.push(cmd.clone());
        if self.entries.len() > MAX_HISTORY {
            self.entries.drain(..self.entries.len() - MAX_HISTORY);
        }
        self.append_to_file(&cmd);
    }

    /// Navigate one step toward older history. Returns the entry to display,
    /// or `None` if already at the oldest matching entry.
    ///
    /// On the first call the current command-line `input` is saved so it can
    /// be restored when the user presses Down past the newest entry.
    pub fn prev(&mut self, current_input: &str) -> Option<&str> {
        if self.browse_index.is_none() {
            self.saved_input = current_input.to_string();
            self.prefix = current_input.to_string();
        }

        let start = self.browse_index.unwrap_or(self.entries.len());
        let prefix = self.prefix.clone();

        // Search backward for the next matching entry.
        let found = self.entries[..start]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.starts_with(&prefix))
            .map(|(i, _)| i);

        if let Some(i) = found {
            self.browse_index = Some(i);
            Some(&self.entries[i])
        } else {
            None
        }
    }

    /// Navigate one step toward newer history. Returns `Ok(entry)` if there
    /// is a newer matching entry, or `Err(())` when stepping past the newest
    /// (caller should restore `saved_input`).
    pub fn next(&mut self) -> Result<&str, ()> {
        let start = match self.browse_index {
            None => return Err(()),
            Some(i) => i + 1,
        };
        let prefix = self.prefix.clone();

        let found = self.entries[start..]
            .iter()
            .enumerate()
            .find(|(_, e)| e.starts_with(&prefix))
            .map(|(i, _)| start + i);

        if let Some(i) = found {
            self.browse_index = Some(i);
            Ok(&self.entries[i])
        } else {
            self.browse_index = None;
            Err(())
        }
    }

    /// Reset all browse state. Call on Enter or Esc.
    pub fn reset_browse(&mut self) {
        self.browse_index = None;
        self.saved_input.clear();
        self.prefix.clear();
    }

    /// Append a single command line to the history file. Creates the parent
    /// directory if needed. All I/O errors are silently ignored so a history
    /// write failure never crashes the editor.
    fn append_to_file(&self, line: &str) {
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Resolve the default history file path following the XDG Base Directory
/// Specification (`XDG_STATE_HOME`, falling back to `~/.local/state`).
pub fn history_path() -> PathBuf {
    if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("rtdvi").join("history");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("rtdvi")
            .join("history");
    }
    PathBuf::from(".local/state/rtdvi/history")
}

/// Read history entries from `path`. Returns an empty vec if the file is
/// missing or unreadable. Blank lines and `#`-prefixed lines are ignored.
/// At most `MAX_HISTORY` entries are kept (oldest are discarded).
pub fn load_entries(path: &Path) -> Vec<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    if entries.len() > MAX_HISTORY {
        entries.drain(..entries.len() - MAX_HISTORY);
    }
    entries
}
