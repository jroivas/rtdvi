# The `:` command line

Press `:` in Normal mode to enter the command line. Type, edit with
`<Backspace>`, press `<Enter>` to run, `<Esc>` to cancel.

## Built-in ex commands

| Command | Aliases | Effect |
|---------|---------|--------|
| `:w` | `:write` | Write current buffer to its path |
| `:w <path>` | | Save as |
| `:wq` | `:x` | Write then quit |
| `:q` | `:quit` | Quit. Fails on unsaved changes. |
| `:q!` | | Force quit (discards changes) |
| `:e <path>` | `:edit`, `:vi`, `:visual` | Open file; reuses existing buffer if one matches |
| `:bnext` | `:bn` | Cycle forward through buffers |
| `:bprev` | `:bp`, `:bprevious` | Cycle backward |
| `:split` | `:sp` | Horizontal split (see [splits-and-tabs.md](splits-and-tabs.md)) |
| `:vsplit` | `:vsp`, `:vs` | Vertical split |
| `:close` | `:clo` | Close the active window |
| `:tabnew [path]` | `:tabe`, `:tabedit` | New tab |
| `:tabnext` | `:tabn` | Next tab |
| `:tabprev` | `:tabp`, `:tabprevious`, `:tabN` | Previous tab |
| `:tab <sub>` | | Dispatcher: `:tab new`, `:tab next`, `:tab prev` |
| `:set <opt>=<val>` | `:setlocal`, `:se` | Per-buffer settings (currently `syntax=` / `filetype=`) |
| `:colorscheme <name>` | `:colo` | Load a colorscheme by name |

`!` suffix forces (e.g. `:q!`, `:w!` — though `:w!` isn't currently
distinguishable from `:w`).

## Tab completion

Press `<Tab>` while typing a `:` command to complete.

### File-path completion

When the cursor is past the first space, `<Tab>` completes
filesystem paths:

- First `<Tab>` inserts the first matching entry (sorted
  alphabetically).
- Second `<Tab>` **opens a popup** listing every match with the
  first one highlighted.
- In the popup:
  - `<Down>` / `<Up>` / `<Tab>` — navigate selection (input updates
    live).
  - `<Enter>` — accept selection and run the command.
  - `<Right>` — accept selection, stay in command mode.
  - `<Left>` — close popup, discard completion state.
  - Any other typed key — close popup, resume normal cmdline editing
    starting from the current selection.
- Directory matches are completed with a trailing `/` so you can keep
  drilling in.
- `~/` is expanded to `$HOME` for that segment.

### Command-name completion

When the cursor is BEFORE the first space (still on the command name),
`<Tab>` completes ex command names from the registry — including
aliases:

```
:vspl<Tab>     → :vsplit          (only match)
:tab<Tab>      → :tab, then <Tab><Tab> opens popup with
                 tab, tabnew, tabe, tabedit, tabnext, tabn,
                 tabprev, tabp, tabprevious, tabN
:tabe<Tab>     → tabe, tabedit
```

Same popup UX as file completion.

### When does completion NOT trigger?

- The partial input matches no entries → `<Tab>` is a no-op (no popup,
  no insertion).
- Typing any character clears the active completion cycle.

## Search line (`/` and `?`)

| Keys | Action |
|------|--------|
| `/`  | Forward search prompt |
| `?`  | Backward search prompt |
| `<Enter>` | Run search; cursor jumps to first match |
| `<Esc>` | Cancel — no pattern stored |
| `<Backspace>` | Edit; empty + backspace cancels |

The pattern is a full Rust `regex` crate regex. The last pattern is
remembered for `n` / `N` in normal mode.

## Custom commands

The `ExCommand` trait is the extension point. Implement it, register
the result via `editor.commands.register(Arc::new(...))` in your
build, and it becomes available at runtime. See
[extending.md](extending.md) for the trait shape and an example.

Plugin-style external commands aren't supported in v1 — there's no
scripting runtime — but the registry pattern means adding the
plumbing later is purely additive.
