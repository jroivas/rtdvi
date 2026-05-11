//! Built-in ex commands. Each is its own zero-state struct so we can
//! register them as `Arc<dyn ExCommand>`. Adding `:foo` = one struct here
//! plus one `register()` call in `Editor::register_builtins`.

use std::sync::Arc;

use super::{CommandError, CommandRegistry, ExArgs, ExCommand};
use crate::Editor;

pub fn register_all(reg: &mut CommandRegistry) {
    reg.register(Arc::new(Quit));
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
