//! Built-in ex commands. Each is its own zero-state struct so we can
//! register them as `Arc<dyn ExCommand>`. Adding `:foo` = one struct here
//! plus one `register()` call in `Editor::register_builtins`.

use std::path::PathBuf;
use std::sync::Arc;

use super::{ArgCompletion, CommandError, CommandRegistry, ExArgs, ExCommand};
use crate::Editor;

pub fn register_all(reg: &mut CommandRegistry) {
    #[cfg(feature = "plugins")]
    reg.register(Arc::new(crate::plugin::PluginDispatch));
    reg.register(Arc::new(Ls));
    reg.register(Arc::new(Paste));
    reg.register(Arc::new(NoPaste));
    reg.register(Arc::new(Quit));
    reg.register(Arc::new(Write));
    reg.register(Arc::new(WriteQuit));
    reg.register(Arc::new(Split));
    reg.register(Arc::new(VSplit));
    reg.register(Arc::new(Term));
    reg.register(Arc::new(Close));
    reg.register(Arc::new(Resize));
    reg.register(Arc::new(Vertical));
    reg.register(Arc::new(Edit_));
    reg.register(Arc::new(BNext));
    reg.register(Arc::new(BPrev));
    reg.register(Arc::new(TabNew));
    reg.register(Arc::new(TabNext));
    reg.register(Arc::new(TabPrev));
    reg.register(Arc::new(TabDispatch));
    reg.register(Arc::new(ColorScheme));
    reg.register(Arc::new(Set));
    reg.register(Arc::new(LspRename));
    reg.register(Arc::new(LspDiagnostic));
    reg.register(Arc::new(LspReferences));
    reg.register(Arc::new(Highlight));
    reg.register(Arc::new(NoHighlight));
    reg.register(Arc::new(ConfigCmd));
    reg.register(Arc::new(Version));
}

struct Ls;
impl ExCommand for Ls {
    fn name(&self) -> &'static str {
        "ls"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["buffers", "files"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        let active_buf = editor.active_buffer_id();
        let mut ids: Vec<crate::buffer::BufferId> = editor.buffers.keys().copied().collect();
        ids.sort_by_key(|b| b.0);

        let mut lines = Vec::with_capacity(ids.len());
        for id in ids {
            let buf = &editor.buffers[&id];
            let active_flag = if Some(id) == active_buf { '%' } else { ' ' };
            let dirty_flag = if buf.is_dirty() { '+' } else { ' ' };
            let name = buf
                .path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "[No Name]".to_string());
            lines.push(format!("{:3} {active_flag}{dirty_flag}  \"{name}\"", id.0));
        }
        editor.status_message = Some(lines.join("\n"));
        Ok(())
    }
}

struct Paste;
impl ExCommand for Paste {
    fn name(&self) -> &'static str {
        "paste"
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        editor.config.options.paste = true;
        editor.status_message = Some("paste".into());
        Ok(())
    }
}

struct NoPaste;
impl ExCommand for NoPaste {
    fn name(&self) -> &'static str {
        "nopaste"
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        editor.config.options.paste = false;
        editor.status_message = Some("nopaste".into());
        Ok(())
    }
}

struct Quit;
impl ExCommand for Quit {
    fn name(&self) -> &'static str {
        "q"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["quit"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let any_dirty = editor.buffers.values().any(|b| b.is_dirty());
        if any_dirty && !args.bang {
            return Err(CommandError::Failed(
                "E37: No write since last change (add ! to override)".into(),
            ));
        }
        editor.should_quit = true;
        Ok(())
    }
}

struct Write;
impl ExCommand for Write {
    fn name(&self) -> &'static str {
        "w"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["write"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        write_active(editor, args.first().map(PathBuf::from))
    }
    fn complete_arg(&self, idx: usize, _: &[String]) -> ArgCompletion {
        if idx == 1 { ArgCompletion::Path } else { ArgCompletion::None }
    }
}

struct WriteQuit;
impl ExCommand for WriteQuit {
    fn name(&self) -> &'static str {
        "wq"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["x"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        write_active(editor, args.first().map(PathBuf::from))?;
        editor.should_quit = true;
        Ok(())
    }
    fn complete_arg(&self, idx: usize, _: &[String]) -> ArgCompletion {
        if idx == 1 { ArgCompletion::Path } else { ArgCompletion::None }
    }
}

struct Split;
impl ExCommand for Split {
    fn name(&self) -> &'static str {
        "split"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["sp"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        crate::window_actions::split_active(editor, crate::window::SplitAxis::Horizontal);
        Ok(())
    }
}

struct VSplit;
impl ExCommand for VSplit {
    fn name(&self) -> &'static str {
        "vsplit"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["vsp", "vs"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        crate::window_actions::split_active(editor, crate::window::SplitAxis::Vertical);
        Ok(())
    }
}

/// `:term [cmd]` — open an embedded terminal in a new horizontal split,
/// vim-style. With no argument it runs the user's `$SHELL`; with an argument
/// it runs `sh -c <args>`. Close with `<C-w>c` or by exiting the shell.
struct Term;
impl ExCommand for Term {
    fn name(&self) -> &'static str {
        "terminal"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["term"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        editor.open_terminal(args.raw.trim());
        Ok(())
    }
}

struct Close;
impl ExCommand for Close {
    fn name(&self) -> &'static str {
        "close"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["clo"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        crate::window_actions::close_active(editor, args.bang)
            .map_err(CommandError::Failed)
    }
}

/// Parse a resize spec (`+N`, `-N`, or absolute `N`) into a row/column delta
/// and apply it to the active window along `axis`.
fn do_resize(
    editor: &mut Editor,
    axis: crate::window::SplitAxis,
    spec: &str,
) -> Result<(), CommandError> {
    use crate::window::SplitAxis;
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(CommandError::BadArgs("usage: :resize +N | -N | N".into()));
    }
    let bad = || CommandError::BadArgs(format!("resize: bad number '{spec}'"));
    let delta = if let Some(n) = spec.strip_prefix('+') {
        n.trim().parse::<i32>().map_err(|_| bad())?
    } else if let Some(n) = spec.strip_prefix('-') {
        -n.trim().parse::<i32>().map_err(|_| bad())?
    } else {
        // Absolute: target size → delta from the current content size.
        let n: i32 = spec.parse().map_err(|_| bad())?;
        let (cw, ch) = crate::window_actions::active_window_size(editor).unwrap_or((0, 0));
        let cur = match axis {
            SplitAxis::Horizontal => ch as i32,
            SplitAxis::Vertical => cw as i32,
        };
        n - cur
    };
    if !crate::window_actions::resize_active(editor, axis, delta) {
        let kind = match axis {
            SplitAxis::Horizontal => "horizontal",
            SplitAxis::Vertical => "vertical",
        };
        editor.status_message = Some(format!("resize: no {kind} split"));
    }
    Ok(())
}

/// `:resize +N` / `:res -N` — grow/shrink the current window's height,
/// moving the boundary it shares with the pane above/below.
struct Resize;
impl ExCommand for Resize {
    fn name(&self) -> &'static str {
        "resize"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["res"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        do_resize(editor, crate::window::SplitAxis::Horizontal, args.raw.trim())
    }
}

/// `:vertical resize +N` / `:vert res -N` — grow/shrink the current window's
/// width. (`:vertical` is only supported in front of `resize` here.)
struct Vertical;
impl ExCommand for Vertical {
    fn name(&self) -> &'static str {
        "vertical"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["vert"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let raw = args.raw.trim();
        let spec = raw
            .strip_prefix("resize")
            .or_else(|| raw.strip_prefix("res"))
            .map(str::trim);
        match spec {
            Some(s) => do_resize(editor, crate::window::SplitAxis::Vertical, s),
            None => Err(CommandError::BadArgs(
                "usage: :vertical resize +N | -N | N".into(),
            )),
        }
    }
    fn complete_arg(&self, idx: usize, _: &[String]) -> ArgCompletion {
        match idx {
            1 => ArgCompletion::Enum(&["resize"]),
            _ => ArgCompletion::None,
        }
    }
}

/// `:version` — report the rtdvi version plus the active plugin runtime
/// (`wasmtime` / `wasmi`) and `mlua`, when those features are compiled in.
struct Version;
impl ExCommand for Version {
    fn name(&self) -> &'static str {
        "version"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["ver", "ve"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        editor.status_message = Some(version_line());
        Ok(())
    }
}

/// A one-line version summary. Only the runtimes actually built into this
/// binary appear (via `#[cfg]`); versions come from `build.rs` reading
/// `Cargo.lock`.
fn version_line() -> String {
    #[allow(unused_mut)] // mutated only when a runtime/lua feature is enabled
    let mut parts = vec![format!("rtdvi {}", env!("CARGO_PKG_VERSION"))];
    #[cfg(feature = "runtime-wasmtime")]
    parts.push(format!(
        "wasmtime {}",
        option_env!("RTDVI_WASMTIME_VERSION").unwrap_or("?")
    ));
    #[cfg(feature = "runtime-wasmi")]
    parts.push(format!(
        "wasmi {}",
        option_env!("RTDVI_WASMI_VERSION").unwrap_or("?")
    ));
    #[cfg(all(feature = "lua-engine", feature = "plugins"))]
    parts.push(format!(
        "mlua {}",
        option_env!("RTDVI_MLUA_VERSION").unwrap_or("?")
    ));
    parts.join("  ")
}

struct Edit_;
impl ExCommand for Edit_ {
    fn name(&self) -> &'static str {
        "e"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["edit", "vi", "visual"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(path) = args.first() else {
            // No argument: reload the current buffer from disk. `:e!` forces
            // past unsaved changes, vim-style.
            let name = editor
                .reload_active_buffer(args.bang)
                .map_err(CommandError::Failed)?;
            editor.status_message = Some(format!("\"{name}\" reloaded from disk"));
            return Ok(());
        };
        let p = std::path::PathBuf::from(path);
        // Reuse existing buffer if open under the same path.
        let existing = editor
            .buffers
            .iter()
            .find(|(_, b)| b.path() == Some(p.as_path()))
            .map(|(id, _)| *id);
        let buf_id = match existing {
            Some(id) => id,
            None => editor
                .open_path(&p)
                .map_err(|e| CommandError::Failed(e.to_string()))?,
        };
        editor.jumplist_record_here();
        if let Some(w) = editor.active_window_mut() {
            w.buffer = buf_id;
            w.cursor = crate::cursor::Cursor::default();
            w.selection = crate::cursor::Selection::None;
            w.top_line = 0;
            w.left_col = 0;
        }
        Ok(())
    }
    fn complete_arg(&self, idx: usize, _: &[String]) -> ArgCompletion {
        if idx == 1 { ArgCompletion::Path } else { ArgCompletion::None }
    }
}

struct BNext;
impl ExCommand for BNext {
    fn name(&self) -> &'static str {
        "bnext"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["bn"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        cycle_buffer(editor, 1)
    }
}

struct BPrev;
impl ExCommand for BPrev {
    fn name(&self) -> &'static str {
        "bprev"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["bp", "bprevious"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        cycle_buffer(editor, -1)
    }
}

fn cycle_buffer(editor: &mut Editor, delta: i32) -> Result<(), CommandError> {
    let mut ids: Vec<crate::buffer::BufferId> = editor.buffers.keys().copied().collect();
    ids.sort_by_key(|b| b.0);
    if ids.is_empty() {
        return Err(CommandError::Failed("no buffers".into()));
    }
    let current = match editor.active_buffer_id() {
        Some(b) => b,
        None => return Err(CommandError::Failed("no active window".into())),
    };
    let pos = ids.iter().position(|b| *b == current).unwrap_or(0);
    let next = if delta >= 0 {
        ids[(pos + 1) % ids.len()]
    } else {
        ids[(pos + ids.len() - 1) % ids.len()]
    };
    if let Some(w) = editor.active_window_mut() {
        w.buffer = next;
        w.cursor = crate::cursor::Cursor::default();
        w.selection = crate::cursor::Selection::None;
        w.top_line = 0;
        w.left_col = 0;
    }
    Ok(())
}

struct TabNew;
impl ExCommand for TabNew {
    fn name(&self) -> &'static str {
        "tabnew"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["tabe", "tabedit"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let buf_id = if let Some(p) = args.first() {
            editor
                .open_path(&std::path::PathBuf::from(p))
                .map_err(|e| CommandError::Failed(e.to_string()))?
        } else {
            editor.open_scratch()
        };
        let win_id = editor.new_window_id();
        let win = crate::window::Window::new(win_id, buf_id);
        editor.windows.insert(win_id, win);
        editor.tabs.push(crate::tab::Tab::single(win_id));
        editor.active_tab = editor.tabs.len() - 1;
        Ok(())
    }
    fn complete_arg(&self, idx: usize, _: &[String]) -> ArgCompletion {
        if idx == 1 { ArgCompletion::Path } else { ArgCompletion::None }
    }
}

struct TabNext;
impl ExCommand for TabNext {
    fn name(&self) -> &'static str {
        "tabnext"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["tabn"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        if editor.tabs.is_empty() {
            return Ok(());
        }
        editor.active_tab = (editor.active_tab + 1) % editor.tabs.len();
        Ok(())
    }
}

struct TabPrev;
impl ExCommand for TabPrev {
    fn name(&self) -> &'static str {
        "tabprev"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["tabp", "tabprevious", "tabN"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        if editor.tabs.is_empty() {
            return Ok(());
        }
        editor.active_tab = (editor.active_tab + editor.tabs.len() - 1) % editor.tabs.len();
        Ok(())
    }
}

/// `:tab {new|next|prev|...} [args]` — dispatches to the matching sub-command.
/// Lets users type the space-separated form (`:tab next`) in addition to the
/// single-word forms (`:tabnext`).
struct TabDispatch;
impl ExCommand for TabDispatch {
    fn name(&self) -> &'static str {
        "tab"
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(sub) = args.first().map(|s| s.to_string()) else {
            return Err(CommandError::BadArgs(
                "usage: :tab {new|next|prev} [args]".into(),
            ));
        };
        // Reconstruct args without the sub-command keyword.
        let rest_words: Vec<String> = args.words.iter().skip(1).cloned().collect();
        let rest_raw = args
            .raw
            .strip_prefix(&sub)
            .map(|s| s.trim_start().to_string())
            .unwrap_or_default();
        let sub_args = ExArgs {
            raw: rest_raw,
            words: rest_words,
            bang: args.bang,
        };
        match sub.as_str() {
            "new" => TabNew.run(editor, &sub_args),
            "next" => TabNext.run(editor, &sub_args),
            "prev" | "previous" => TabPrev.run(editor, &sub_args),
            "close" => crate::window_actions::close_active(editor, args.bang)
                .map_err(CommandError::Failed),
            other => Err(CommandError::BadArgs(format!(
                "tab: unknown sub-command {other:?} (expected new/next/prev)"
            ))),
        }
    }
    fn complete_arg(&self, idx: usize, before: &[String]) -> ArgCompletion {
        match idx {
            1 => ArgCompletion::Enum(&["close", "new", "next", "prev"]),
            2 => match before.first().map(String::as_str) {
                Some("new") => ArgCompletion::Path,
                _ => ArgCompletion::None,
            },
            _ => ArgCompletion::None,
        }
    }
}

/// `:set <option>[=<value>]` — runtime configuration.
///
/// Boolean shorthands (vim-compatible, `no` prefix disables):
///   `number`/`nu`           — line numbers in gutter
///   `autoindent`/`ai`       — copy indent of previous line on new line
///   `smartindent`/`si`      — language-aware extra indent rules
///   `expandtab`/`et`        — Tab key inserts spaces
///
/// Value options:
///   `tabstop=N`/`ts=N`      — tab display width and indent size
///   `shiftwidth=N`/`sw=N`   — indent size for >> and smartindent (alias for tabstop)
///   `syntax=NAME`/`ft=NAME` — per-buffer filetype override
struct Set;
impl ExCommand for Set {
    fn name(&self) -> &'static str {
        "set"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["setlocal", "se"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        if args.words.is_empty() {
            return Err(CommandError::BadArgs("usage: :set option=value".into()));
        }
        for word in &args.words {
            let (key, value) = match word.split_once('=') {
                Some(p) => p,
                None => {
                    match word.as_str() {
                        "syntax" | "syn" | "filetype" | "ft" => {
                            let cur = editor
                                .active_buffer_id()
                                .and_then(|b| editor.buffers.get(&b))
                                .and_then(|b| b.syntax_override())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "<auto>".into());
                            editor.status_message = Some(format!("syntax={cur}"));
                        }
                        "number" | "nu" => {
                            editor.config.options.number = true;
                            editor.status_message = Some("number".into());
                        }
                        "nonumber" | "nonu" => {
                            editor.config.options.number = false;
                            editor.status_message = Some("nonumber".into());
                        }
                        "autoindent" | "ai" => {
                            editor.config.options.autoindent = true;
                            editor.status_message = Some("autoindent".into());
                        }
                        "noautoindent" | "noai" => {
                            editor.config.options.autoindent = false;
                            editor.status_message = Some("noautoindent".into());
                        }
                        "smartindent" | "si" => {
                            editor.config.options.smartindent = true;
                            editor.status_message = Some("smartindent".into());
                        }
                        "nosmartindent" | "nosi" => {
                            editor.config.options.smartindent = false;
                            editor.status_message = Some("nosmartindent".into());
                        }
                        "paste" => {
                            editor.config.options.paste = true;
                            editor.status_message = Some("paste".into());
                        }
                        "nopaste" => {
                            editor.config.options.paste = false;
                            editor.status_message = Some("nopaste".into());
                        }
                        "expandtab" | "et" => {
                            editor.config.options.expandtab = true;
                            editor.status_message = Some("expandtab".into());
                        }
                        "noexpandtab" | "noet" => {
                            editor.config.options.expandtab = false;
                            editor.status_message = Some("noexpandtab".into());
                        }
                        "nocolorcolumn" | "nocc" => {
                            editor.config.options.color_column.clear();
                            editor.status_message = Some("nocolorcolumn".into());
                        }
                        _ => {
                            editor.status_message =
                                Some(format!("set: unknown option '{word}'"));
                        }
                    }
                    continue;
                }
            };
            match key {
                "syntax" | "syn" | "filetype" | "ft" => {
                    let Some(buf_id) = editor.active_buffer_id() else {
                        return Err(CommandError::Failed("no active buffer".into()));
                    };
                    // `on`/`auto` → auto-detect (no override).
                    // `off`/`none`/empty → disable highlighting entirely.
                    // anything else → force that filetype.
                    let (override_val, msg) = match value.trim().to_ascii_lowercase().as_str() {
                        "on" | "auto" => (None, "syntax=auto (auto-detect)".to_string()),
                        "off" | "none" | "" => {
                            (Some("off".to_string()), "syntax=off (disabled)".to_string())
                        }
                        _ => {
                            let n = crate::syntax::normalize_filetype(value);
                            (Some(n.clone()), format!("syntax set to '{n}'"))
                        }
                    };
                    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
                        buf.set_syntax_override(override_val);
                    }
                    editor.invalidate_syntax_cache(Some(buf_id));
                    editor.status_message = Some(msg);
                }
                "number" | "nu" => {
                    editor.config.options.number = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}number", if editor.config.options.number { "" } else { "no" }
                    ));
                }
                "autoindent" | "ai" => {
                    editor.config.options.autoindent = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}autoindent", if editor.config.options.autoindent { "" } else { "no" }
                    ));
                }
                "smartindent" | "si" => {
                    editor.config.options.smartindent = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}smartindent", if editor.config.options.smartindent { "" } else { "no" }
                    ));
                }
                "expandtab" | "et" => {
                    editor.config.options.expandtab = parse_bool(value);
                    editor.status_message = Some(format!(
                        "{}expandtab", if editor.config.options.expandtab { "" } else { "no" }
                    ));
                }
                "tabstop" | "ts" | "tab_width" | "shiftwidth" | "sw" => {
                    match value.parse::<usize>() {
                        Ok(n) if n >= 1 => {
                            editor.config.options.tab_width = n;
                            editor.status_message = Some(format!("tabstop={n}"));
                        }
                        _ => {
                            editor.status_message =
                                Some(format!("set: tabstop must be a positive integer, got '{value}'"));
                        }
                    }
                }
                "colorcolumn" | "cc" | "color_column" => {
                    // `:set cc=` (empty value) turns the ruler off.
                    editor.config.options.color_column = value.to_string();
                    editor.status_message = Some(format!("colorcolumn={value}"));
                }
                _ => {
                    editor.status_message =
                        Some(format!("set: unknown option '{key}'"));
                }
            }
        }
        Ok(())
    }
}

/// Parse a bool from a `:set option=<value>` string.
/// Accepts: `on`/`off`, `true`/`false`, `1`/`0`, `yes`/`no`.
fn parse_bool(s: &str) -> bool {
    matches!(s, "on" | "true" | "1" | "yes")
}

struct ColorScheme;
impl ExCommand for ColorScheme {
    fn name(&self) -> &'static str {
        "colorscheme"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["colo"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(name) = args.first() else {
            // No arg = report current name.
            let cur = if editor.colorscheme.name.is_empty() {
                "<default>"
            } else {
                editor.colorscheme.name.as_str()
            };
            editor.status_message = Some(format!("colorscheme: {cur}"));
            return Ok(());
        };
        match crate::colorscheme::load(name) {
            Ok(scheme) => {
                editor.status_message = Some(format!("colorscheme {name} loaded"));
                editor.colorscheme = scheme;
                Ok(())
            }
            Err(e) => Err(CommandError::Failed(format!("E185: Cannot find color scheme '{name}': {e}"))),
        }
    }
}

/// `:LspRename <new_name>` — request `textDocument/rename` and apply the
/// returned `WorkspaceEdit` to every affected open buffer (and to disk
/// for files not currently open). Lasts one undo step per buffer.
struct LspRename;
impl ExCommand for LspRename {
    fn name(&self) -> &'static str {
        "LspRename"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["lsprename"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(new_name) = args.first().map(|s| s.to_string()) else {
            return Err(CommandError::BadArgs(
                "usage: :LspRename <new_name>".into(),
            ));
        };
        let edit = crate::lsp_apply::request_rename(editor, &new_name);
        match edit {
            Some(we) => {
                let n = crate::lsp_apply::apply_workspace_edit(editor, &we);
                editor.status_message = Some(format!("LSP: renamed in {n} files"));
                Ok(())
            }
            None => Err(CommandError::Failed("LSP: rename failed".into())),
        }
    }
}

struct LspDiagnostic;
impl ExCommand for LspDiagnostic {
    fn name(&self) -> &'static str {
        "LspDiagnostic"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["lspdiag"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        if let Some(action) = editor.actions.lookup("lsp_diagnostic_at_cursor") {
            action(editor);
        }
        Ok(())
    }
}

/// `:LspReferences` — repeat `gr`. Useful when you want to refresh the
/// stored reference list without remembering the keybind.
struct LspReferences;
impl ExCommand for LspReferences {
    fn name(&self) -> &'static str {
        "LspReferences"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["lspref"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        if let Some(action) = editor.actions.lookup("lsp_references") {
            action(editor);
        }
        Ok(())
    }
}

/// `:highlight <text>` — add (or toggle off) a persistent highlight for
/// `<text>`. Colour is auto-assigned from a small curated palette;
/// when the palette is exhausted, the oldest highlight is dropped to
/// free its colour slot.
struct Highlight;
impl ExCommand for Highlight {
    fn name(&self) -> &'static str {
        "highlight"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["hl"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        if args.raw.is_empty() {
            return Err(CommandError::BadArgs("usage: :highlight <text>".into()));
        }
        let text = args.raw.trim_end();
        match editor.highlights.toggle_literal(text) {
            crate::highlights::ToggleResult::Added(t) => {
                editor.status_message = Some(format!("highlight: +{t}"));
            }
            crate::highlights::ToggleResult::Removed(t) => {
                editor.status_message = Some(format!("highlight: -{t}"));
            }
            crate::highlights::ToggleResult::BadPattern => {
                return Err(CommandError::Failed("highlight: bad pattern".into()));
            }
        }
        Ok(())
    }
}

/// `:nohighlight [<text>]` — remove the highlight for `<text>`, or
/// clear every highlight when called with no arguments.
struct NoHighlight;
impl ExCommand for NoHighlight {
    fn name(&self) -> &'static str {
        "nohighlight"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["nohl"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        if args.raw.is_empty() {
            editor.highlights.clear();
            editor.status_message = Some("highlight: cleared all".into());
        } else {
            let text = args.raw.trim_end();
            if editor.highlights.remove_literal(text) {
                editor.status_message = Some(format!("highlight: -{text}"));
            } else {
                editor.status_message = Some(format!("highlight: no match for {text:?}"));
            }
        }
        Ok(())
    }
}

/// `:config <sub>` — runtime tooling for the user config:
///
/// - `:config show [toml|json]` — print the active config (current
///   format by default).
/// - `:config path` — print the loaded-from path and the search list.
/// - `:config convert <toml|json> [path]` (alias `conv`) — write the
///   active config in the chosen format. Path defaults to the
///   canonical `config.<ext>` next to the loaded file (or the XDG
///   default if running on built-in defaults).
/// - `:config load [path]` — reload the active path, or load a new
///   file from `path` (format inferred from extension).
struct ConfigCmd;

impl ExCommand for ConfigCmd {
    fn name(&self) -> &'static str {
        "config"
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        use crate::config::loader::{self, Format};
        let Some(sub) = args.words.first().map(|s| s.to_string()) else {
            return Err(CommandError::BadArgs(
                "usage: :config {show|path|convert|conv|load} [args]".into(),
            ));
        };
        let rest: Vec<String> = args.words.iter().skip(1).cloned().collect();
        match sub.as_str() {
            "show" => {
                let fmt = match rest.first() {
                    Some(s) => Format::parse_name(s).ok_or_else(|| {
                        CommandError::BadArgs(format!(
                            "config show: unknown format {s:?} (expected toml/json)"
                        ))
                    })?,
                    None => editor
                        .config_path
                        .as_deref()
                        .and_then(Format::from_path)
                        .unwrap_or(Format::Toml),
                };
                let text = loader::serialize(&editor.config, fmt)
                    .map_err(|e| CommandError::Failed(format!("config show: {e}")))?;
                open_config_view(editor, &text, fmt);
            }
            "path" => {
                let mut out = String::new();
                match editor.config_path.as_ref() {
                    Some(p) => out.push_str(&format!("loaded: {}\n", p.display())),
                    None => out.push_str("loaded: <defaults — no config file found>\n"),
                }
                out.push_str("searched:\n");
                for p in loader::search_paths() {
                    out.push_str(&format!(
                        "  {} ({})\n",
                        p.display(),
                        if p.exists() { "exists" } else { "missing" }
                    ));
                }
                editor.status_message = Some(out.trim_end().to_string());
            }
            "convert" | "conv" => {
                let Some(fmt_arg) = rest.first() else {
                    return Err(CommandError::BadArgs(
                        "usage: :config convert <toml|json> [path]".into(),
                    ));
                };
                let fmt = Format::parse_name(fmt_arg).ok_or_else(|| {
                    CommandError::BadArgs(format!(
                        "config convert: unknown format {fmt_arg:?} (expected toml/json)"
                    ))
                })?;
                let dest = if let Some(p) = rest.get(1) {
                    PathBuf::from(p)
                } else {
                    // Default destination: same directory as the currently
                    // loaded file, switching the extension. Falls back to
                    // the canonical default-write path.
                    let base = editor
                        .config_path
                        .clone()
                        .unwrap_or_else(|| {
                            loader::default_path().unwrap_or_else(|| PathBuf::from("config.toml"))
                        });
                    base.with_extension(fmt.extension())
                };
                match loader::write_to_path(&editor.config, &dest) {
                    Ok(()) => {
                        editor.status_message = Some(format!(
                            "config: wrote {} ({fmt})",
                            dest.display()
                        ));
                    }
                    Err(e) => return Err(CommandError::Failed(format!("config convert: {e}"))),
                }
            }
            "load" => {
                let target = if let Some(p) = rest.first() {
                    PathBuf::from(p)
                } else {
                    match editor.config_path.clone() {
                        Some(p) => p,
                        None => {
                            return Err(CommandError::Failed(
                                "config load: no config currently loaded, pass a path".into(),
                            ));
                        }
                    }
                };
                match loader::load_or_default(&target) {
                    Ok(cfg) => {
                        editor.apply_config(cfg);
                        editor.config_path = Some(target.clone());
                        editor.status_message =
                            Some(format!("config: loaded {}", target.display()));
                    }
                    Err(e) => return Err(CommandError::Failed(format!("config load: {e}"))),
                }
            }
            other => {
                return Err(CommandError::BadArgs(format!(
                    "config: unknown sub-command {other:?} (expected show/path/convert/load)"
                )));
            }
        }
        Ok(())
    }
    fn complete_arg(&self, idx: usize, before: &[String]) -> ArgCompletion {
        match idx {
            1 => ArgCompletion::Enum(&["conv", "convert", "load", "path", "show"]),
            2 => match before.first().map(String::as_str) {
                Some("show" | "convert" | "conv") => ArgCompletion::Enum(&["json", "toml"]),
                Some("load") => ArgCompletion::Path,
                _ => ArgCompletion::None,
            },
            3 => match before.first().map(String::as_str) {
                Some("convert" | "conv") => ArgCompletion::Path,
                _ => ArgCompletion::None,
            },
            _ => ArgCompletion::None,
        }
    }
}

/// Create a scratch buffer containing `text`, open it in a fresh
/// vertical split, and apply the matching filetype so syntax
/// highlighting kicks in (when a `toml.vim` / `json.vim` is available
/// on the system).
fn open_config_view(editor: &mut Editor, text: &str, fmt: crate::config::loader::Format) {
    let buf_id = editor.open_scratch();
    if let Some(buf) = editor.buffers.get_mut(&buf_id) {
        let _ = buf.insert(0, text);
        buf.set_syntax_override(Some(match fmt {
            crate::config::loader::Format::Toml => "toml".to_string(),
            crate::config::loader::Format::Json => "json".to_string(),
        }));
    }
    crate::window_actions::split_active(editor, crate::window::SplitAxis::Vertical);
    if let Some(w) = editor.active_window_mut() {
        w.buffer = buf_id;
        w.cursor = crate::cursor::Cursor::default();
        w.top_line = 0;
        w.left_col = 0;
        w.selection = crate::cursor::Selection::None;
    }
    editor.status_message = Some(format!("config: showing ({fmt}) in new split"));
}

fn write_active(editor: &mut Editor, path_arg: Option<PathBuf>) -> Result<(), CommandError> {
    let Some(buf_id) = editor.active_buffer_id() else {
        return Err(CommandError::Failed("no active buffer".into()));
    };
    let result = {
        let buf = editor
            .buffers
            .get_mut(&buf_id)
            .ok_or_else(|| CommandError::Failed("buffer disappeared".into()))?;
        match path_arg {
            Some(p) => buf.save_as(&p),
            None => buf.save(),
        }
    };
    match result {
        Ok(()) => {
            // Read back the path now that the borrow ended.
            let path = editor
                .buffers
                .get(&buf_id)
                .and_then(|b| b.path())
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            editor.status_message = Some(format!("\"{path}\" written"));
            crate::event::emit(editor, crate::event::Event::BufferSaved(buf_id));
            Ok(())
        }
        Err(e) => Err(CommandError::Failed(e.to_string())),
    }
}
