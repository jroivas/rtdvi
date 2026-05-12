# Text highlights

Persistent in-buffer highlights — vim's `:Mark` / `:highlight` flow,
self-contained (no plugin needed). Useful for keeping an eye on a
variable while you trace it through a file.

## Usage

| Command / key | Effect |
|---------------|--------|
| `:highlight <text>` | Add a highlight for `<text>` (regex-literal). Toggles off on a second call. |
| `:hl <text>` | Short alias. |
| `:nohighlight <text>` | Remove the highlight for `<text>`. |
| `:nohighlight` | Clear every highlight. |
| `:nohl` | Short alias. |
| `\m` | Toggle a word-bounded highlight on the word under the cursor. |

`\m` is the default leader-prefixed binding — `\` is the default
leader, `m` is the suffix. Set `options.leader = ","` in your TOML
and bind `<leader>m` if you prefer `,m`.

## How matching works

There are two flavours of pattern, and they're kept in sync so
the same word can't accumulate duplicate entries:

- `:highlight foo` → compiled regex matches `foo` anywhere
  (substring). Will paint `foo`, `foobar`, and `xfoox`.
- `\m` on `foo` → compiled regex `\bfoo\b` (word-bounded). Will
  paint `foo` and `a foo b` but NOT `foobar`.

If you `:highlight foo` and then move the cursor onto another `foo`
and press `\m`, the existing literal entry is **removed** (not
duplicated). The same goes the other way: `\m` first, then
`:highlight foo` removes the word-bounded entry. `:nohighlight foo`
matches either form.

## Colour palette

Ten curated colours, chosen to stay readable against both light and
dark terminal backgrounds:

```
gold  sky-blue  hot-pink  pale-green  orange
plum  powder-blue  lemon-chiffon  light-pink  light-blue
```

Each new highlight gets the next unused slot. Once all ten are in
use, the next addition drops the **oldest** entry and reuses its
slot — same UX as vim-mark.

## Rendering priority

When multiple style layers want the same cell, the priority (highest
wins) is:

1. Active selection (visual / visual-line / visual-block)
2. Whitespace marks (trailing/tabs, see
   [whitespace-marks.md](whitespace-marks.md))
3. `:highlight` overlay
4. Syntax highlighting
5. Default text style

So a highlighted symbol still looks selected when you `v`-select it,
and trailing-whitespace red wins over a `:highlight` painted on a
trailing match (the rare edge case).

## Implementation

- [`src/highlights.rs`](../src/highlights.rs) — the `Highlights`
  struct, palette, and toggle logic.
- [`src/highlight_actions.rs`](../src/highlight_actions.rs) —
  the `\m` action.
- `command/builtin.rs` — `HighlightCmd` and `NoHighlightCmd`.
- `ui/window_render.rs` — applies the overlay per line.
