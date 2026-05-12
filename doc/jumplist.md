# Jumplist

A chronological stack of "interesting" cursor positions, so you can
`<C-o>` back to where you came from after any jump.

## Keys

| Keys | Effect |
|------|--------|
| `<C-o>` | Go to the previous entry. Records current position as a forward target so `<Tab>` can return. |
| `<Tab>` | Go forward through the list. (Terminals send `<Tab>` for `Ctrl-I`, vim's traditional forward-jump key.) |

## What gets recorded

A jump is pushed before any of these:

- LSP navigation: `gd`, `gD`, `gi`, `gf`, `gr`
- Search jumps: `/`, `?` accepting a pattern
- Symbol search: `*`, `£`, `#`
- Fuzzy finder: `:ff` opening a result
- Absolute-line `G` / `gg` (e.g. `42G`, `gg`)

Step-wise motions (`j`, `k`, `w`, `<C-f>`, …) are NOT recorded — the
list stays useful as a "places I really meant to go" rather than a
trace of every move.

## Coalescing

Repeated jumps within the **same line** of the same buffer collapse
into the most recent entry. Hopping `/foo` → `/foo` (next match on
the same line) doesn't litter the list with duplicates.

## Capacity

100 entries max. When full, the oldest entry is dropped.

## Per-editor, not per-window

The list is global to the editor session; jumping to a different
file is just another entry. `<C-o>` switches windows / tabs back to
wherever the entry lived. The window where you press `<C-o>`
becomes the window the jump lands in.

## Implementation

- [`src/jumplist.rs`](../src/jumplist.rs) — `Jumplist`, `JumpEntry`,
  `record` / `back` / `forward`.
- [`src/jump_actions.rs`](../src/jump_actions.rs) — `jump_back`,
  `jump_forward`, plus `*` / `£` / `#` (`search_word_under_cursor`,
  `search_word_under_cursor_backward`) and `zz` / `zt` / `zb`
  (centre / top / bottom anchoring).
- Every site that pushes a jump calls `editor.jumplist_record_here()`
  right before the actual position change.
