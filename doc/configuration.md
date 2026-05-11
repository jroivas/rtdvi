# Configuration

jvim reads a single TOML file. Search order:

1. `$JVIM_CONFIG` (if set, treated as a file path)
2. `$XDG_CONFIG_HOME/jvim/config.toml`
3. `$HOME/.config/jvim/config.toml`

If none of these exist, defaults apply and jvim runs silently.

## Schema

```toml
# Runtime options (mostly editing knobs).
[options]
tab_width = 4         # default: 4
expandtab = false     # default: false (tab key inserts a literal tab)
number    = false     # default: false; show line numbers in gutter

# Filetype globs. Tried BEFORE the built-in extension table.
# Right-hand side is either a vim-style filetype name OR a MIME type.
[filetypes]
"*.cpp"       = "c++"           # vim-style, normalised to "cpp"
"*.md"        = "text/markdown" # MIME, normalised to "markdown"
"Cargo.toml"  = "rust"          # exact-name overrides
"*.zsh"       = "sh"
"BUILD"       = "make"

# Keymaps. Each entry binds a key sequence to an action in a single mode.
# `keys` uses the vim notation: `<C-x>`, `<Esc>`, `<Enter>`, plain chars.
[[keymaps]]
mode = "normal"
keys = "<Space>w"
action = "delete_word_forward"

[[keymaps]]
mode = "visual"
keys = "y"
action = "visual_yank"

# Modes accepted: "normal" / "n", "insert" / "i", "visual" / "v",
# "vline" / "V", "vblock" / "C-v".

# Language servers. One block per server. The block name is the server
# name (used as the cache key, so two filetypes pointing at the same
# config = same process).
[lsp.clangd]
cmd = ["clangd", "-j=2", "--background-index"]
filetypes = ["c", "cpp", "objc", "objcpp"]
root_markers = [".git", "compile_commands.json", "compile_flags.txt"]

[lsp.rust-analyzer]
cmd = ["rust-analyzer"]
filetypes = ["rust"]
# root_markers defaults to [".git"] if omitted.
```

## What the options do

| Field            | Default | Meaning |
|------------------|---------|---------|
| `options.tab_width` | 4 | Display width of `\t` characters. Cursor math uses this. |
| `options.expandtab` | false | Reserved; not yet honoured by the insert path. |
| `options.number`    | false | Show line numbers in a left gutter. |
| `filetypes` (map)   | `{}` | Glob → filetype/MIME override. Longest pattern wins. |
| `keymaps` (array)   | `[]` | Extra key bindings layered on top of the defaults. |
| `lsp` (map)         | `{}` | One block per server; see [lsp.md](lsp.md). |

## Filetype rules in detail

For every buffer, jvim picks the filetype in this order:

1. The buffer's own override (set by `:set syntax=…`) — wins
   unconditionally.
2. A glob in `[filetypes]` matching the basename of the buffer's path
   (longest pattern first).
3. The built-in basename/extension table (`Cargo.toml` → rust,
   `Makefile` → make, `*.rs` → rust, …).
4. `mime_guess` extension lookup mapped back to a filetype.
5. `"generic"` fallback.

Both vim-style names (`c++`, `cpp`, `js`) and MIME types
(`text/markdown`, `text/x-c++src`) are accepted as right-hand sides
and normalised internally.

## Keymap reference

Built-in action names you can bind via `[[keymaps]]`. Full list of
actions is in [`src/`](../src/) — search for `reg.register("…", …)`.

Selected highlights:

- Motions: `move_left/right/up/down`, `word_forward/backward/end`,
  `line_start`, `line_end`, `first_line`, `last_line`, `page_up`,
  `page_down`, `match_bracket`, `section_forward`, `section_backward`.
- Insert: `enter_insert_before/after/line_start/line_end`,
  `open_line_below/above`.
- Delete: `delete_line`, `delete_line_down/up`, `delete_word_forward/backward/end`,
  `delete_to_line_end/start`, `delete_to_buffer_end/start`,
  `delete_char`, `delete_char_before`.
- Yank: `yank_line`, `yank_line_down/up`, `yank_word_forward/backward/end`,
  `yank_to_line_end/start`, `yank_to_buffer_end/start`.
- Visual: `enter_visual`, `enter_visual_line`, `enter_visual_block`,
  `visual_delete`, `visual_yank`, `visual_change`, `paste_after`,
  `block_insert_at_left`, `block_append_at_right`.
- Window: `split_horizontal`, `split_vertical`, `close_window`,
  `focus_left/right/up/down`, `focus_next`, `equalize_splits`.
- Replace: `enter_replace`.
- Search: `search_forward`, `search_backward`, `search_next`,
  `search_prev`.
- LSP: `lsp_goto_definition`, `lsp_hover`, `lsp_diagnostic_next`,
  `lsp_diagnostic_prev`.

## Reload

Reloading the config at runtime isn't implemented in v1. Restart the
editor to pick up changes.
