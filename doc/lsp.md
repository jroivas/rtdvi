# LSP

rtdvi ships with a **native LSP client**. By default it auto-spawns
clangd on C/C++ buffers. Add `[lsp.NAME]` blocks in your config to
register more servers.

## Default setup

Opening a `.c`/`.cpp`/`.h`/`.hpp` file in a directory whose ancestors
contain any of `.git`, `compile_commands.json`, or `compile_flags.txt`
triggers:

```
clangd -j=2 --background-index --background-index-priority=low
        --malloc-trim --pch-storage=disk
```

If `clangd` isn't on PATH, the spawn fails silently and the editor
runs without LSP for that buffer. Check `editor.log` for the warning.

## Keybindings

| Keys        | Action |
|-------------|--------|
| `gd`        | Go to definition (picker if multiple results) |
| `gD`        | Go to declaration (picker if multiple results) |
| `gi`        | Go to implementation (picker if multiple results) |
| `gf`        | Go to type definition (picker if multiple results) |
| `gr`        | List references — picker when >1, direct jump for exactly one |
| `K`         | Hover. First non-empty line of the response is shown in the cmdline. |
| `]d`        | Next diagnostic line in this buffer (wraps). |
| `[d`        | Previous diagnostic line (wraps). |
| `\h`        | Show the diagnostic message at the cursor in the cmdline. |
| `\rn`       | Open the command line pre-filled with `:LspRename ` to rename the symbol. |

All work in Normal mode. `\` is the default leader — see
[Configuration](configuration.md#the-leader-key) to change it.

The "go to" variants reuse an existing buffer when one already
references the target file, and the cursor ends up **centered in the
window** so you can immediately read the context around the landing site
(same as a manual `zz`).

When a goto or references request returns more than one location, a
**picker popup** appears. Navigate with `j`/`k` or the arrow keys,
press Enter to jump to the selected entry, or Esc/`q`/Ctrl-C to cancel.

Before any of these jumps, the current position is pushed onto the
[jumplist](motions.md#jumplist) so `<C-o>` brings you back.

### Customising LSP keybindings

All LSP actions can be rebound via `[[keymaps]]` in your config. To use
a different leader or different keys:

```toml
[options]
leader = ","   # use comma as leader instead of backslash

# Override the diagnostic and rename bindings to match your preference.
[[keymaps]]
mode   = "normal"
keys   = "<leader>h"          # expands to ",h" with the leader above
action = "lsp_diagnostic_at_cursor"

[[keymaps]]
mode   = "normal"
keys   = "<leader>rn"         # expands to ",rn"
action = "lsp_rename_prompt"  # enters `:LspRename ` in the command line
```

If you prefer to drive rename directly from an ex command:

```toml
[[keymaps]]
mode   = "normal"
keys   = "<leader>rn"
action = ":LspRename "        # runs `:LspRename <name>` — fill in the name
```

## Ex commands

| Command | Effect |
|---------|--------|
| `:LspRename <new>` | Send `textDocument/rename` for the symbol under the cursor with the given new name. The returned `WorkspaceEdit` is applied across every affected buffer (and unopened files) as a single undo entry per file. |
| `:LspReferences`   | Send `textDocument/references`; results show in the cmdline. |
| `:LspDiagnostic`   | Print the diagnostic message at the cursor (handy when the gutter marker is cryptic). |

Aliases: `:lsprename`, `:lspref`, `:lspdiag`.

## Diagnostics

When a server publishes `textDocument/publishDiagnostics`, rtdvi:

- Stores them on the client (keyed by URI).
- Renders a **gutter column** with a one-char severity marker on
  every diagnostic line:
  - `!` red — Error
  - `?` yellow — Warning
  - `i` blue — Information
  - `h` cyan — Hint

When multiple diagnostics fall on the same line, the highest-severity
marker wins.

The gutter only appears when there's at least one diagnostic — it
doesn't clutter unhighlighted buffers.

### Virtual text (inline diagnostics)

Enable `diagnostic_virtual_text` in `[options]` to show the diagnostic
message appended after the end of each affected line:

```toml
[options]
diagnostic_virtual_text = true
```

Example output:

```
#include <stdio.h>  ■ Included header stdio.h is not used directly (fix available)
```

The `■` marker uses the same severity colour as the gutter symbol. The
message text is rendered in dim gray. Only the first line of each
message is shown; when multiple diagnostics land on the same line the
highest-severity one wins. Disabled by default.

## Configuration

```toml
[lsp.clangd]
cmd = ["clangd", "-j=2", "--background-index"]
filetypes = ["c", "cpp", "objc", "objcpp"]
root_markers = [".git", "compile_commands.json", "compile_flags.txt"]

[lsp.rust-analyzer]
cmd = ["rust-analyzer"]
filetypes = ["rust"]
# root_markers defaults to [".git"]

[lsp.pyright]
cmd = ["pyright-langserver", "--stdio"]
filetypes = ["python"]
root_markers = [".git", "pyproject.toml", "setup.py"]
```

Defining your own `[lsp.clangd]` **replaces** the built-in default
(useful for tweaking flags). Other servers are additive.

## Architecture

One **`Client`** per `(server_name, workspace_root)` pair:

```
   crossterm event loop (main thread)
        │
        ▼
   Editor::lsp_did_open(buffer)
        │
        ├── Manager::ensure(filetype, path)
        │       │
        │       ▼
        │    Client::spawn(cmd) ──── std::process::Command
        │                                 ├─ stdin  ─── synchronous writes from main thread
        │                                 ├─ stdout ─── reader thread
        │                                 └─ stderr ─── /dev/null
        │
        └── Client::did_open(uri, language_id, text)
                                                 ▼
                                         mpsc::channel
                                                 ▼
                                         Editor::lsp_poll()  ←─── called every render
```

A background **reader thread** parses framed JSON-RPC messages off
stdout and sends them as `InboundMessage` values down an mpsc
channel. The main thread drains the channel on every render
(`Editor::lsp_poll`), turning notifications into editor state
(diagnostics).

For requests like `gd` / `K`, the main thread blocks on the channel
for up to a configurable timeout (1.5s by default), processing any
notifications that arrive in the meantime, until the matching
response shows up.

## Message flow per buffer

1. `:e foo.c` opens the buffer.
2. `Editor::lsp_did_open` finds (or spawns) the right client, sends
   `textDocument/didOpen` with the file's text and language ID.
3. Server starts indexing. Diagnostics trickle in via
   `publishDiagnostics`.
4. User presses `gd` → `Client::goto_definition(uri, line, col)` →
   synchronous `textDocument/definition` request → if one location,
   cursor jumps; if multiple, picker popup opens.

## `didChange`

Every buffer edit emits a `BufferChanged` event, which triggers a
full-text `didChange` notification to the running LSP client. The
server therefore always sees the live in-memory content, and
diagnostics update as you type without needing to save first.

Full-text sync (sending the entire file on each change) is used
rather than incremental deltas — simpler and compatible with every
server. For typical file sizes the overhead is negligible.

## Shutdown

When the editor exits, every running client gets `shutdown` + `exit`
notifications and the child is reaped. If you `:q!` quickly, the
process kill in `Drop` ensures nothing's left hanging.

## Adding more actions

The existing actions live in
[`src/lsp_actions.rs`](../src/lsp_actions.rs). Each follows the same
pattern: read cursor position + URI, find the client, fire a
synchronous request, act on the result.

Already wired up: `gd`, `gD`, `gi`, `gf`, `gr`, `K`,
`\h`, `\rn`, `:LspRename`, `:LspReferences`, `:LspDiagnostic`, `]d`/`[d`.

Easy follow-ups (same plumbing, ~20 lines each):

- `<leader>gf` → `textDocument/formatting` (needs `TextEdit` apply)
- `textDocument/codeAction` (needs a list UI + apply)
- `textDocument/signatureHelp` (needs a floating popup)

## Testing without clangd

`tests/m34_lsp.rs` uses a tiny Python script as a mock LSP server.
It implements just enough of the protocol to respond to `initialize`,
emit diagnostics on `didOpen`, return canned `definition` and `hover`
results, and exit cleanly on `shutdown` / `exit`. Useful as a
template if you want to test your own server-specific behaviour
locally.
