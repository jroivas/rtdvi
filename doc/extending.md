# Extending rtdvi

rtdvi deliberately routes everything through small **registries** so
the surface for adding new features is well-defined. There's no
embedded scripting runtime in v1, but every extension seam is
designed so a Lua/WASM/dynamic-library layer could plug in later
without rewriting the core.

The four registries:

1. **`KeymapRegistry`** — per-mode trie of key sequences → `Action`.
2. **`ActionRegistry`** — name → `Fn(&mut Editor)` closure.
3. **`CommandRegistry`** — name → `Arc<dyn ExCommand>`.
4. **`EventBus`** — `Event` enum + `Listener` trait.

## Adding a normal-mode command

Suppose you want `<leader>q` to close the current window without
saving. The workflow:

```rust
use std::sync::Arc;
use rtdvi::Editor;
use rtdvi::keymap::{Action, ActionRegistry, KeymapRegistry};
use rtdvi::mode::ModeId;

fn close_window_force(editor: &mut Editor) {
    rtdvi::window_actions::close_active(editor);
}

pub fn register(actions: &mut ActionRegistry, keymap: &mut KeymapRegistry) {
    actions.register("close_window_force", Arc::new(close_window_force));
    keymap
        .bind(ModeId::Normal, "<Space>q", Action::Builtin("close_window_force"))
        .unwrap();
}
```

Plug `register(&mut editor.actions, &mut editor.keymap)` into
`Editor::register_builtins`.

The same mechanism is how every built-in (`motion`, `delete_actions`,
`yank_actions`, …) wires itself in. Find the pattern in
[`src/motion.rs`](../src/motion.rs) — `register_all` + `bind_default_keys`.

## Adding an ex command

```rust
use std::sync::Arc;
use rtdvi::command::{CommandError, CommandRegistry, ExArgs, ExCommand};
use rtdvi::Editor;

struct Echo;
impl ExCommand for Echo {
    fn name(&self) -> &'static str { "echo" }
    fn aliases(&self) -> &'static [&'static str] { &[] }
    fn run(&self, editor: &mut Editor, args: &ExArgs) -> Result<(), CommandError> {
        editor.status_message = Some(args.raw.clone());
        Ok(())
    }
}

pub fn register(reg: &mut CommandRegistry) {
    reg.register(Arc::new(Echo));
}
```

Now `:echo hello world` shows `hello world` in the cmdline area.

`ExArgs` carries:
- `raw: String` — everything after the command name (and any `!`).
- `words: Vec<String>` — whitespace-split tokens.
- `bang: bool` — true when the user typed `:cmd!`.

For multi-word dispatchers (`:tab new`, `:tab next`), see how
`TabDispatch` in `src/command/builtin.rs` peels off the first word
and forwards to sub-commands.

## Listening for buffer events

```rust
use rtdvi::event::{emit, Event, Listener};
use rtdvi::Editor;

struct AutoSave;
impl Listener for AutoSave {
    fn on_event(&mut self, editor: &mut Editor, ev: &Event<'_>) {
        if let Event::BufferChanged { buffer, .. } = ev {
            // Schedule a save…
            let _ = (editor, buffer);
        }
    }
}

editor.events.subscribe(Box::new(AutoSave));
```

`Event` is `#[non_exhaustive]` so adding a new variant is
non-breaking. Existing variants:

- `BufferChanged { buffer, edit }`
- `CursorMoved { window }`
- `ModeChanged { from, to }`
- `BufferOpened(buffer)`
- `BufferSaved(buffer)`
- `WindowResized`
- `Quit`

Notes:

- Listeners receive `&mut Editor` so they can react by mutating
  state. The bus uses a swap pattern internally so the borrow
  checker is happy.
- Listeners that emit further events during dispatch are not
  reprocessed in v1 — the queue is taken, drained, and restored.

## Custom modes

In principle you'd add a new `ModeId` variant and a `handle_key`
implementation, then bind keys under that mode. In practice modes
themselves aren't a hot extension surface — the keymap registry covers
"per-mode key behaviour" well enough that most features don't need
new modes.

## Building a feature: walkthrough

A concrete example end-to-end — replicating the `:set syntax=`
override (the actual code in
[`src/command/builtin.rs`](../src/command/builtin.rs)):

1. **Data**: add `syntax_override: Option<String>` to `Buffer`.
2. **API**: `Buffer::set_syntax_override`.
3. **Wiring**: `Editor::syntax_for(buffer)` reads the override (if
   any) before consulting the config-glob layer.
4. **Command**: implement `ExCommand for Set` that parses
   `syntax=NAME` and calls `buf.set_syntax_override(Some(normalised))`.
5. **Invalidation**: call `editor.invalidate_syntax_cache(Some(buf_id))`
   so the next render rebuilds with the new filetype.
6. **Tests**: open a `.rs` file, run `:set syntax=c`, assert
   `editor.syntax_for(id).filetype == "c"`.

Each step is small, the failure modes are obvious, and nothing else
in the editor cares.

## Where to read code

```
src/editor.rs            top-level state — all registries hang off here
src/keymap/mod.rs        Key, Action, KeymapRegistry, ActionRegistry
src/keymap/trie.rs       trie resolver (Matched / Pending / None)
src/command/mod.rs       CommandRegistry, ExCommand trait
src/event.rs             EventBus, Listener, Event enum
src/buffer.rs            text + undo transactions
src/syntax.rs            filetype detection + builtin rules
src/colorscheme.rs       .vim parser, vim defaults, terminal preference
src/lsp/                 client / manager / transport for the LSP layer
```

Tests in `tests/m*.rs` are also a great way to see how each subsystem
is meant to be driven.
