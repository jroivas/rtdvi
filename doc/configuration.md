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
expandtab = true      # default: true; tab key inserts spaces to next tab stop
                      # (Shift+Tab always inserts a literal tab regardless)
number    = false     # default: false; show line numbers in gutter
leader    = "\\"      # default: "\\"; the <leader> key in user keymaps
highlight_trailing_whitespace = false  # default: false; paint trailing spaces/tabs red
highlight_tabs                = false  # default: false; paint every tab cell red

# System-clipboard register: "<this>yy yanks to the OS clipboard,
# "<this>p pastes from it. Default is "q". Set to a space to disable.
system_clipboard_register = "q"
# Optional explicit clipboard commands. When omitted, jvim auto-detects:
#   $WAYLAND_DISPLAY  → wl-copy / wl-paste --no-newline
#   $DISPLAY          → xclip -selection clipboard [ -o ]
#   macOS             → pbcopy / pbpaste
# clipboard_copy_cmd  = ["wl-copy"]
# clipboard_paste_cmd = ["wl-paste", "--no-newline"]

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
| `options.expandtab` | true | If true, pressing Tab in insert mode inserts spaces up to the next multiple of `tab_width`. If false, inserts a literal `\t`. Shift+Tab always inserts a literal `\t` regardless of this setting — a manual escape hatch for files (Makefiles, Go) where you genuinely need a tab. |
| `options.number`    | false | Show line numbers in a left gutter. |
| `options.leader`    | `\` | Character `<leader>` expands to in user `[[keymaps]]` entries. Set to `","` or `" "` to taste. |
| `options.highlight_trailing_whitespace` | false | Paint the run of spaces/tabs after the last non-whitespace char on a line with a red background. See [whitespace-marks.md](whitespace-marks.md). |
| `options.highlight_tabs` | false | Paint every tab character (anywhere on the line) with a red background. Useful in spaces-only projects. |
| `options.system_clipboard_register` | `'q'` | The `"<letter>` register that routes yank/paste through the system clipboard. See [registers.md](registers.md). Set to `' '` (space) to disable. |
| `options.clipboard_copy_cmd`  | `None` | Command + args that copy stdin to the OS clipboard. `None` ⇒ auto-detect (`wl-copy`, `xclip`, `pbcopy`). |
| `options.clipboard_paste_cmd` | `None` | Command + args that print the OS clipboard. `None` ⇒ auto-detect (`wl-paste --no-newline`, `xclip -o`, `pbpaste`). |
| `filetypes` (map)   | `{}` | Glob → filetype/MIME override. Longest pattern wins. |
| `keymaps` (array)   | `[]` | Extra key bindings layered on top of the defaults. |
| `lsp` (map)         | `{}` | One block per server; see [lsp.md](lsp.md). |

## The `<leader>` key

`<leader>` (or `<Leader>` — case-insensitive) in a `keys` value is
expanded to `options.leader` at config-load time. Vim's default is
`\`; that's also jvim's default, which is why `\m` toggles the
under-cursor highlight out of the box.

```toml
[options]
leader = ","

[[keymaps]]
mode = "normal"
keys = "<leader>m"
action = "highlight_toggle_word_under_cursor"
# After leader expansion this binds ",m".
```

Multi-character leaders work but aren't recommended — the trie
resolver treats them as a literal sequence, so a `leader = "gp"`
would shadow Vim's `gp` motion.

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
- LSP: `lsp_goto_definition`, `lsp_goto_declaration`,
  `lsp_goto_implementation`, `lsp_goto_type_definition`,
  `lsp_references`, `lsp_hover`, `lsp_diagnostic_next`,
  `lsp_diagnostic_prev`, `lsp_diagnostic_at_cursor`.
- Jumplist / symbol search: `jump_back`, `jump_forward`,
  `search_word_under_cursor`, `search_word_under_cursor_backward`,
  `center_cursor`, `scroll_cursor_top`, `scroll_cursor_bottom`.
- Highlights: `highlight_toggle_word_under_cursor`.

## Reload

Reloading the config at runtime isn't implemented in v1. Restart the
editor to pick up changes.
