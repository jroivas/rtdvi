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
  `:highlight` / syntax.
