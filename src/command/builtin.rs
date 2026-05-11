//! Built-in ex commands. Each is its own zero-state struct so we can
//! register them as `Arc<dyn ExCommand>`. Adding `:foo` = one struct here
//! plus one `register()` call in `Editor::register_builtins`.

use std::path::PathBuf;
use std::sync::Arc;

use super::{CommandError, CommandRegistry, ExArgs, ExCommand};
use crate::Editor;

pub fn register_all(reg: &mut CommandRegistry) {
    reg.register(Arc::new(Quit));
    reg.register(Arc::new(Write));
    reg.register(Arc::new(WriteQuit));
    reg.register(Arc::new(Split));
    reg.register(Arc::new(VSplit));
    reg.register(Arc::new(Close));
    reg.register(Arc::new(Edit_));
    reg.register(Arc::new(BNext));
    reg.register(Arc::new(BPrev));
    reg.register(Arc::new(TabNew));
    reg.register(Arc::new(TabNext));
    reg.register(Arc::new(TabPrev));
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

struct Close;
impl ExCommand for Close {
    fn name(&self) -> &'static str {
        "close"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["clo"]
    }
    fn run(&self, editor: &mut Editor, _args: &ExArgs) -> Result<(), CommandError> {
        crate::window_actions::close_active(editor);
        Ok(())
    }
}

struct Edit_;
impl ExCommand for Edit_ {
    fn name(&self) -> &'static str {
        "e"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["edit"]
    }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        let Some(path) = args.first() else {
            return Err(CommandError::BadArgs("usage: :e <path>".into()));
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
        if let Some(w) = editor.active_window_mut() {
            w.buffer = buf_id;
            w.cursor = crate::cursor::Cursor::default();
            w.selection = crate::cursor::Selection::None;
            w.top_line = 0;
            w.left_col = 0;
        }
        Ok(())
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
