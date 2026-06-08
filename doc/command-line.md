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
| `:set <opt>[=<val>]` | `:setlocal`, `:se` | Runtime options — see table below |
| `:colorscheme <name>` | `:colo` | Load a colorscheme by name |

### `:set` options

Boolean options accept the bare name to enable, `no` prefix to disable, or
`=on`/`=off`/`=true`/`=false`/`=1`/`=0`.

| Option | Aliases | Default | Description |
|--------|---------|---------|-------------|
| `number` | `nu` | off | Line numbers in left gutter |
| `autoindent` | `ai` | on | Copy previous line's indent on Enter / `o` / `O` |
| `smartindent` | `si` | on | Language-aware extra indent (requires `autoindent`) |
| `expandtab` | `et` | on | Tab key inserts spaces; off = literal `\t` |
| `tabstop=N` | `ts`, `sw`, `shiftwidth` | 4 | Tab display width and indent step |
| `syntax=NAME` | `ft`, `filetype` | auto | Per-buffer filetype override (`:set syntax=off` to disable) |
| `:ff [query]` | | Interactive fuzzy file finder. See [fzf.md](fzf.md). `:ff!` busts the cache. |
| `:highlight <text>` | `:hl` | Toggle a persistent text highlight. See [highlights.md](highlights.md). |
| `:nohighlight [<text>]` | `:nohl` | Remove one highlight, or clear all if no arg. |
| `:LspRename <new>` | `:lsprename` | Rename the symbol under the cursor via the LSP server. |
| `:LspDiagnostic` | `:lspdiag` | Show the diagnostic at the cursor in the cmdline. |
| `:LspReferences` | `:lspref` | List references to the symbol under the cursor. |
| `:config show [fmt]` | | Print the active config in TOML or JSON. |
| `:config path` | | Print the loaded-from path and search list. |
| `:config convert <fmt> [path]` | `:config conv` | Write the active config in the chosen format. |
| `:config load [path]` | | Reload the active file, or load a different one. |

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
  drilling in. Pressing `<Tab>` again on `:vi src/` descends into
  `src/` and lists its contents (instead of just re-showing `src/`).
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

### How each command declares its argument completion

Every command's `ExCommand::complete_arg` returns an `ArgCompletion`
value describing what Tab should resolve at a given positional
slot. The variants:

- `None` — Tab is a no-op (zero-arg commands like `:q`, `:bnext`).
- `Path` — filesystem completion (`:e`, `:w`, `:tabnew`).
- `Enum(&[…])` — fixed list (`:config show <Tab>` → `json` / `toml`).
- `Dynamic(fn)` — runtime-resolved list, for things like a future
  `:colorscheme <Tab>` that needs editor state.

The completion module dispatches through the registry, so adding a
new ex command with custom completion is a single-file change: add
the struct, override `complete_arg`, register it. Existing examples
worth copying from:

```
:config <Tab>      → conv | convert | load | path | show
:config sh<Tab>    → show
:config show <Tab> → json | toml
:config load <Tab> → filesystem paths
:tab <Tab>         → close | new | next | prev
:tab new <Tab>     → filesystem paths
```

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

## Substitution (`:s`)

```
:[range]s/pattern/replacement/[flags]
```

Regex search-and-replace, using the **same Rust `regex` syntax** as the
`/` search line above.

**Ranges** (the `[range]` prefix):

| Range | Lines affected |
|-------|----------------|
| *(none)* | The current line |
| `%` | The whole buffer |
| `'<,'>` | The last visual selection (see below) |
| `N` | Line `N` (1-based) |
| `N,M` | Lines `N` through `M` |
| `.` / `$` | Current line / last line (combine, e.g. `.,$`) |

**Flags** (after the closing `/`):

- `g` — replace **every** match on each line, not just the first.
- `i` — case-insensitive match.

**Replacement** understands vim-style back-references: `\1`…`\9` insert
capture groups and `&` inserts the whole match. A literal `$` is inserted
verbatim. An empty pattern (`:%s//new/`) reuses the last search pattern.

```
:%s/foo/bar/g          replace every "foo" with "bar", whole buffer
:s/\s\+$//             strip trailing whitespace on the current line
:2,10s/old/new/        only lines 2–10, first match per line
:%s/(\w+)=(\w+)/\2=\1/ swap around the "=" using capture groups
:%s/cat/[&]/g          wrap every "cat" in brackets → "[cat]"
```

### Substituting over a visual selection

Press `:` while a visual selection is active and the command line opens
**prefilled with `'<,'>`** — exactly like vim. Just append the
substitution:

```
V  j  j        select three lines
:              command line shows  :'<,'>
s/old/new/g    →  :'<,'>s/old/new/g
<Enter>        applies only to those lines
```

The `'<` / `'>` line range is captured the moment you press `:`, so it
holds even though entering the command line clears the on-screen
highlight.

## Custom commands

The `ExCommand` trait is the extension point. Implement it, register
the result via `editor.commands.register(Arc::new(...))` in your
build, and it becomes available at runtime. See
[extending.md](extending.md) for the trait shape and an example.

Plugin-style external commands aren't supported in v1 — there's no
scripting runtime — but the registry pattern means adding the
plumbing later is purely additive.
