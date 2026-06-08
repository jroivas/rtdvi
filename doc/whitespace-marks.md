# Whitespace marks

Two opt-in flags that paint problematic whitespace red, so you spot
it before it gets committed.

## Configuration

```toml
[options]
highlight_trailing_whitespace = true
highlight_tabs                = true
```

Both default to `false`. They're independent — set just the one you
want.

## `highlight_trailing_whitespace`

Paints every space or tab in the **run** of whitespace after the
last non-whitespace character of a line. A `\r` immediately before
`\n` (CR-LF stragglers) is also considered trailing whitespace.

Lines that are **only** whitespace are entirely red.

```
foo···        ← three trailing spaces, all red
foo bar       ← no trailing whitespace, no red
······        ← whole line is red
```

## `highlight_tabs`

Paints every tab cell red, wherever the tab appears on the line.
Each tab expands to `options.tab_width` cells (defaulting to 4) and
all of them are painted.

```
→→→→foo       ← red tab expansion
··→→bar       ← spaces stay normal; tab at col 2-3 red
foo→bar       ← tab in the middle, still red
```

Useful in spaces-only projects where stray tabs are bugs.

## Both at once

Set both flags and a trailing tab (or a line of only tabs) is
highlighted by either rule — they don't conflict.

## Column ruler

`color_column` highlights one or more screen columns with a background
ruler — vim's `colorcolumn` — so you can see when a line is running past
your `textwidth`.

```toml
[options]
textwidth    = 80
color_column = "80"        # mark column 80
# color_column = "80,100"  # several columns
# color_column = "+1"      # column just past textwidth (i.e. 81 here)
```

The value is a comma-separated list of **1-based** columns. An entry with
a leading `+`/`-` is taken relative to `textwidth`, so `"+1"` marks the
first column you shouldn't type into. The default is the empty string,
which turns the ruler off.

Unlike the whitespace marks, the ruler is painted across the **whole
height** of each real text line — including the empty cells past the end
of a short line — so it reads as a continuous vertical guide. It is not
drawn on the `~` rows below the end of the buffer.

It can also be toggled at runtime, vim-style:

```
:set colorcolumn=80      (aliases: :set cc=80, :set color_column=80)
:set colorcolumn=        clear (also :set nocolorcolumn / :set nocc)
```

## Rendering priority

Whitespace marks sit just below the active selection but above
`:highlight` and syntax highlighting (see
[highlights.md](highlights.md#rendering-priority)). That means
selected cells render as selected; everything else where a mark
fires renders red.

## Why not always on?

Some files legitimately use trailing whitespace (Markdown's
two-space-then-newline for hard breaks, generated test fixtures,
etc.). Opt-in keeps the editor from turning into Christmas lights
for users who haven't asked for it.

## Implementation

- [`src/config/mod.rs`](../src/config/mod.rs) — the two `Options`
  fields.
- [`src/ui/window_render.rs`](../src/ui/window_render.rs) —
  `build_whitespace_overlay` produces a per-cell style map; the
  line renderer applies it after selection but before
  `:highlight` / syntax. `color_columns` parses the ruler spec and
  the column background is painted straight onto the frame buffer
  after the text, so it spans the full line height.
