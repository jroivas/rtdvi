//! Tab completion for the `:` command line.
//!
//! Vim-style file-path completion: when Tab is pressed, the partial path
//! ending at the cursor is replaced with the first matching filesystem
//! entry; subsequent Tabs cycle through the remaining matches. Any key
//! other than Tab resets the cycle (see `mode/command.rs`).
//!
//! The command name itself (before the first space) is not completed in
//! v1 — it's a smaller surface and easy to type by hand.

use std::fs;
use std::path::PathBuf;

use crate::Editor;

#[derive(Default, Debug, Clone)]
pub struct CompletionState {
    /// Input as it was BEFORE the first Tab — cycling rebuilds from this so
    /// we don't compound replacements.
    pub original_input: String,
    pub original_cursor: usize,
    /// Byte offset in `original_input` where the partial path started.
    pub prefix_start: usize,
    pub matches: Vec<String>,
    pub index: usize,
    /// `true` after the second Tab in a sequence — the wildmenu popup is
    /// visible and arrow keys navigate within it.
    pub popup_visible: bool,
}

pub fn handle_tab(editor: &mut Editor) {
    // We already have completion state — Tab either reveals the popup
    // (second Tab), advances the selection (third+ Tab once popup is
    // up), OR — special case — descends into a directory when the
    // first Tab autocompleted a single directory entry.
    if let Some(comp) = &mut editor.command_line.completion {
        if comp.matches.is_empty() {
            return;
        }
        // Single-directory completion: the user just landed on a
        // trailing `/` after typing e.g. `:vi src` → `:vi src/`. The
        // next Tab should list files INSIDE that directory, not pop
        // up a one-row "src/" entry. Drop the cached state and let
        // the fresh-parse branch below pick up the new input.
        let descend = !comp.popup_visible
            && comp.matches.len() == 1
            && comp.matches[0].ends_with('/');
        if descend {
            editor.command_line.completion = None;
            // fall through to the build-match-list block below
        } else {
            comp.popup_visible = true;
            comp.index = (comp.index + 1) % comp.matches.len();
            apply_current_match(&mut editor.command_line);
            return;
        }
    }

    // First Tab — parse the partial and build the match list.
    let input = editor.command_line.input.clone();
    let cursor = editor.command_line.cursor.min(input.len());
    let prefix_start = find_partial_start(&input, cursor);
    let partial: String = input[prefix_start..cursor].to_string();

    // Position-based source: command name at the head, then dispatch
    // to the matched command's `complete_arg` for each subsequent
    // positional argument.
    let matches = if prefix_start == 0 {
        find_command_completions(editor, &partial)
    } else {
        find_arg_completions(editor, &input, prefix_start, &partial)
    };
    if matches.is_empty() {
        return;
    }

    // Single match: fill it immediately, no popup needed.
    // Multiple matches: fill the first candidate AND open the popup right
    // away so the user can see all options without a second Tab.
    let popup_visible = matches.len() > 1;
    let new_input = format!("{}{}", &input[..prefix_start], &matches[0]);
    editor.command_line.cursor = new_input.len();
    editor.command_line.input = new_input;
    editor.command_line.completion = Some(CompletionState {
        original_input: input,
        original_cursor: cursor,
        prefix_start,
        matches,
        index: 0,
        popup_visible,
    });
}

/// Move the popup selection up one (wraps at the top).
pub fn nav_up(editor: &mut Editor) {
    if let Some(comp) = &mut editor.command_line.completion {
        if comp.matches.is_empty() {
            return;
        }
        comp.index = if comp.index == 0 {
            comp.matches.len() - 1
        } else {
            comp.index - 1
        };
    }
    apply_current_match(&mut editor.command_line);
}

/// Move the popup selection down one (wraps at the bottom).
pub fn nav_down(editor: &mut Editor) {
    if let Some(comp) = &mut editor.command_line.completion {
        if comp.matches.is_empty() {
            return;
        }
        comp.index = (comp.index + 1) % comp.matches.len();
    }
    apply_current_match(&mut editor.command_line);
}

/// Accept the current selection: hide the popup but keep the input as-is.
pub fn accept(editor: &mut Editor) {
    if let Some(comp) = &mut editor.command_line.completion {
        comp.popup_visible = false;
    }
}

/// Close the popup without changing the current input. The completion
/// cycle is also cleared (left-arrow signals "I'm done browsing").
pub fn close_popup(editor: &mut Editor) {
    editor.command_line.completion = None;
}

fn apply_current_match(state: &mut crate::mode::command::CommandLineState) {
    let (new_input, new_cursor) = match state.completion.as_ref() {
        Some(comp) if !comp.matches.is_empty() => {
            let s = format!(
                "{}{}",
                &comp.original_input[..comp.prefix_start],
                &comp.matches[comp.index]
            );
            let cursor = s.len();
            (s, cursor)
        }
        _ => return,
    };
    state.input = new_input;
    state.cursor = new_cursor;
}

/// Walk backward from `cursor` to find the start of the current "word"
/// (whitespace-delimited). Returns the byte offset where the partial begins.
fn find_partial_start(input: &str, cursor: usize) -> usize {
    let bytes = &input.as_bytes()[..cursor];
    let mut i = cursor;
    while i > 0 {
        let prev = i - 1;
        if bytes[prev] == b' ' || bytes[prev] == b'\t' {
            return i;
        }
        i -= 1;
    }
    0
}

/// Arg-position completion. Tokenises what's already typed, looks up
/// the command in the registry, and asks it (via
/// [`crate::command::ExCommand::complete_arg`]) what kind of value
/// goes at this position. Each command owns its own arg contract —
/// no per-command logic lives in this module.
fn find_arg_completions(
    editor: &Editor,
    input: &str,
    prefix_start: usize,
    partial: &str,
) -> Vec<String> {
    let words: Vec<String> = input[..prefix_start]
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let Some(cmd_name) = words.first() else {
        return Vec::new();
    };
    let arg_idx = words.len(); // 1-based: words.len() is the arg being typed.
    let before: Vec<String> = words.iter().skip(1).cloned().collect();

    let Some(cmd) = editor.commands.lookup(cmd_name) else {
        // Unknown command — nothing useful to suggest.
        return Vec::new();
    };
    match cmd.complete_arg(arg_idx, &before) {
        crate::command::ArgCompletion::None => Vec::new(),
        crate::command::ArgCompletion::Path => find_path_completions(partial),
        crate::command::ArgCompletion::Enum(opts) => filter_prefix(partial, opts),
        crate::command::ArgCompletion::Dynamic(f) => f(editor, partial),
    }
}

fn filter_prefix(partial: &str, candidates: &[&str]) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| c.starts_with(partial))
        .map(|c| (*c).to_string())
        .collect()
}

fn find_command_completions(editor: &Editor, partial: &str) -> Vec<String> {
    editor.commands.complete_names(partial)
}

fn find_path_completions(partial: &str) -> Vec<String> {
    let (dir_str, name_prefix): (&str, &str) = match partial.rfind('/') {
        Some(pos) => (&partial[..=pos], &partial[pos + 1..]),
        None => ("", partial),
    };
    let dir_path = expand_dir(dir_str);
    let Ok(read) = fs::read_dir(&dir_path) else {
        return Vec::new();
    };

    let mut matches: Vec<String> = read
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.starts_with(name_prefix) {
                return None;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let suffix = if is_dir { "/" } else { "" };
            Some(format!("{dir_str}{name}{suffix}"))
        })
        .collect();
    matches.sort();
    matches
}

fn expand_dir(dir: &str) -> PathBuf {
    if dir.is_empty() {
        return PathBuf::from(".");
    }
    if dir == "~" || dir == "~/" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = dir.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest.trim_end_matches('/'));
        }
    }
    PathBuf::from(dir)
}
