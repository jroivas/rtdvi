# Splits, windows, and tabs

## Splits

| Ex command | Effect |
|------------|--------|
| `:split` / `:sp` | Horizontal split: new window appears **above** the current one and gets focus |
| `:vsplit` / `:vsp` / `:vs` | Vertical split: new window appears **left** of the current one and gets focus |
| `:close` / `:clo` | Close the active window. Collapses parent split into the sibling. Last window of last tab = quit. |

Or with the `<C-w>` chord:

| Keys                | Equivalent ex command |
|---------------------|-----------------------|
| `<C-w>s` / `<C-w><C-s>` | `:split` |
| `<C-w>v` / `<C-w><C-v>` | `:vsplit` |
| `<C-w>c` / `<C-w><C-c>` | `:close` |

Both forms work — held-Ctrl through both keys is equivalent to
release-Ctrl-then-letter, the same way vim handles them.

## Window navigation

| Keys | Action |
|------|--------|
| `<C-w>h` / `<C-w><C-h>` | Focus window to the left |
| `<C-w>j` / `<C-w><C-j>` | Focus window below |
| `<C-w>k` / `<C-w><C-k>` | Focus window above |
| `<C-w>l` / `<C-w><C-l>` | Focus window to the right |
| `<C-w>w` / `<C-w><C-w>` | Cycle to next window |

The directional commands **always step exactly one column/row** at a
time — they never skip past nearer windows. Within the target strip,
the window that contains the cursor's screen position is selected. If
the cursor sits exactly on a split boundary, the window **above** (or
to the **left**) of the boundary wins.

## Equalising splits

`<C-w>=` rebalances every split node in the current tab so each side
gets space proportional to how many same-axis leaves it contains.
Concretely: if you have three columns at the top level, each one gets
≈1/3 of the width regardless of how many rows are nested inside.
Then within each column, the rows equalise among themselves.

## Maximising a window

| Keys | Action |
|------|--------|
| `<C-w>_` / `<C-w><C-_>` | Maximise the active window's **height** |
| `<C-w>\|` | Maximise the active window's **width** |

Each pushes the active window as large as the layout allows along one
axis, shrinking its neighbours toward the minimum — while leaving the
*other* axis untouched. So `<C-w>_` grows a stacked window to fill the
column without disturbing the column widths, and `<C-w>|` widens a
side-by-side window without changing row heights. `<C-w>=` puts
everything back to equal.

## Resizing splits manually

| Command | Effect |
|---------|--------|
| `:resize +N` / `:res +N` | Grow the current window's **height** by N rows |
| `:resize -N` | Shrink the current window's height by N rows |
| `:resize N` | Set the current window's height to N rows (absolute) |
| `:vertical resize +N` / `:vert res +N` | Grow the current window's **width** by N columns |
| `:vertical resize -N` | Shrink the current window's width by N columns |
| `:vertical resize N` | Set the current window's width to N columns |

Resizing moves the boundary the current window shares with the
neighbour on that side — growing the current window shrinks the
neighbour, and vice-versa. `:resize` works on the nearest **horizontal**
split (stacked windows); `:vertical resize` on the nearest **vertical**
split (side-by-side). If there's no split along that axis, the command
does nothing.

**Minimum sizes.** No pane ever drops below **10%** of its split. Within
that, shrinking a side has *soft stops*: a single `:vertical resize`
won't take the shrinking pane below the next tier down — 20, then 10,
then 5 columns — so a big shrink stops at the tier rather than
collapsing the neighbour. Small adjustments within a tier are free.
Horizontal panes always keep at least one content row plus their
statusline.

## Tabs

| Ex command | Effect |
|------------|--------|
| `:tabnew` / `:tabe` / `:tabedit` | New tab with a scratch buffer (or `:tabnew <file>`) |
| `:tabnext` / `:tabn` | Cycle to next tab |
| `:tabprev` / `:tabp` / `:tabN` | Previous tab |
| `:tab new\|next\|prev` | Space-separated dispatcher form |

Keymap:

| Keys | Action |
|------|--------|
| `gt` | Next tab |
| `gT` | Previous tab |
| `<C-w>T` | Move the active window into its own new tab |

`<C-w>T` peels the active window out of its current tab, collapsing the
split it leaves behind into the sibling, and drops it into a fresh tab
placed right after the current one. It's a no-op when the window is
already the only one in its tab.

A tab line appears at the top of the screen when there are 2+ tabs.

## Per-window statusline

Every window has its own status row at the bottom of its rect — buffer
name, dirty flag, cursor row:col. The **active** window's statusline
is highlighted with a brighter background and shows the mode badge
(`NORMAL`, `INSERT`, etc.); inactive ones are muted.

## Split layout details

The split tree is a binary tree of horizontal / vertical splits with
ratios. Splits are stored on each `Tab`. A `Window` is a leaf —
buffer + cursor + viewport + last-known render size.

After every split / close, the tab's tree is rewritten:

- `:split` replaces the focused leaf with `Split { axis: H, ratio:
  0.5, first: new_win, second: old_win }` — new window goes on top.
- `:vsplit` is the same but with `axis: V`, new window on the left.
- `:close` removes the leaf and collapses the parent split into the
  sibling.

`<C-w>=` walks the tree and resets every ratio based on the
same-axis leaf count of each subtree, as described above.
