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
