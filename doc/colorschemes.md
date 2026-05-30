# Colorschemes

rtdvi reads **vim `.vim` colorscheme files** directly. No custom format.

## Switching

```
:colorscheme desert
:colo myfault2
```

`:colorscheme` with no argument prints the currently-loaded name.

## Search paths

For `:colorscheme NAME`, rtdvi looks for `NAME.vim` in this order:

1. `./colors/NAME.vim` (CWD)
2. `$XDG_CONFIG_HOME/rtdvi/colors/NAME.vim`
3. `$HOME/.config/rtdvi/colors/NAME.vim`
4. `/usr/share/vim/vim*/colors/NAME.vim` (whatever's installed)

The first match wins. The system path means you get vim's bundled
schemes (`desert`, `default`, `evening`, `morning`, `slate`, …) for
free if you have vim installed.

## Default scheme at startup

`main.rs` tries `myfault2`, then `desert`, then `default` — first one
found wins. If none of those exist, rtdvi falls back to the **built-in
vim defaults** (everything still gets sensible colors).

## File format rtdvi understands

Only `highlight` directives — every other vim-script line (`let`,
`set`, `if`, `endif`, `syntax …`, …) is silently skipped.

### Simple group definition

```vim
highlight Comment   ctermfg=cyan       guifg=#80a0ff term=bold cterm=bold
highlight Search    ctermbg=3          guibg=#c0c000
highlight Statement ctermfg=yellow                  cterm=bold
```

Recognised keys:

| Key       | Value examples |
|-----------|----------------|
| `ctermfg` | Color name (`red`, `darkblue`, `lightcyan`, `gray`, `brown`, …) OR a number 0–255 |
| `ctermbg` | Same as `ctermfg` |
| `guifg`   | `#rrggbb`, or a cterm color name as fallback |
| `guibg`   | Same |
| `cterm` / `gui` / `term` | Comma-separated attrs: `bold`, `italic`, `underline`, `reverse`/`inverse` |

`NONE` / `bg` / empty values are ignored, so old vim schemes with
`gui=NONE` don't accidentally clobber anything.

### Linking

```vim
highlight default link rustKeyword Statement
hi link cIncluded cString
```

`hi link FROM TO` aliases one group to another. Vim's `default` /
`def` keywords are accepted. Chains of links are followed (depth-capped
at 16 to defend against malformed files).

### `hi clear`

```vim
hi clear
```

Wipes every group and link defined so far in this scheme. Useful when
a scheme starts by clearing inherited definitions.

## cterm vs gui — which wins?

When both `ctermfg=…` and `guifg=…` are supplied for the same group,
rtdvi picks based on the `$COLORTERM` env var:

- `COLORTERM=truecolor` or `COLORTERM=24bit` → `guifg` (RGB) wins.
- Anything else → `ctermfg` (16-color palette) wins.

This means schemes work on basic terminals too, instead of emitting
RGB escapes that get ignored. If your terminal supports 24-bit color
and you want the richer scheme, export `COLORTERM=truecolor`.

## Vim defaults are always applied

After parsing a scheme, rtdvi layers `syncolor.vim`'s default group
definitions and standard `SynLink` mappings on top — only filling in
groups the user's scheme didn't define. This is critical because most
schemes only define a handful of groups (`Comment`, `Search`, …) and
rely on vim's defaults for the rest (`Statement`, `Type`, `Constant`,
…).

Defaults wired in:

- **Standard links**: `String→Constant`, `Number→Constant`,
  `Boolean→Constant`, `Function→Identifier`, `Conditional→Statement`,
  `Repeat→Statement`, `Label→Statement`, `Keyword→Statement`,
  `Operator→Statement`, `Exception→Statement`, `Include→PreProc`,
  `Define→PreProc`, `Macro→PreProc`, `PreCondit→PreProc`,
  `StorageClass→Type`, `Structure→Type`, `Typedef→Type`,
  `Tag→Special`, `SpecialChar→Special`, `Delimiter→Special`,
  `SpecialComment→Special`, `Debug→Special`.
- **Base group colors** (dark-bg variant from vim's `syncolor.vim`):
  `Comment`, `Constant`, `Special`, `Identifier`, `Statement`,
  `PreProc`, `Type`, `Underlined`, `Error`, `Todo`, `Title`, `Search`.

So even a one-line `highlight Comment …` user scheme produces a fully
styled editor.

## Writing your own scheme

The bundled `colors/myfault2.vim`:

```vim
" Vim color file
hi clear Normal
set bg&
hi clear
if exists("syntax_on")
  syntax reset
endif
let colors_name = "myfault2"
highlight Search    ctermbg=3       guibg=#c0c000
highlight Comment   term=bold cterm=bold ctermfg=cyan  guifg=#80a0ff
```

You don't need the `let`/`set`/`if`/`syntax reset` boilerplate for
rtdvi's parser — they're harmless but ignored. The minimal scheme is
just a handful of `highlight` lines.
