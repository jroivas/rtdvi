# markdown — rtdvi render-buffer plugin

Adds `:md`, which renders the **current buffer** as styled markdown in a new,
non-editable split (a "render buffer"), with **followable links**.

## Build & install

```sh
rustup target add wasm32-unknown-unknown   # once
make install                               # build + copy to ~/.local/rtdvi/plugins/
```

Then enable it in `~/.config/rtdvi/config.toml`:

```toml
plugins = ["markdown"]
```

## Use

Open a `.md` file and run `:md`. In the rendered split:

- `<Enter>` — follow the link under the cursor (`[text](other.md)` opens and
  renders `other.md` in the same pane).
- `<Tab>` / `<S-Tab>` — jump to the next / previous link.
- `j` / `k` / `Ctrl-d` / `Ctrl-u` — scroll. `q` or `:q` — close.

Re-run `:md` to refresh after editing the source.

## What it shows

ATX (`#`..`######`) and setext (`===`/`---`) headings, **bold**, *italic*,
`inline code`, ~~strikethrough~~, fenced code blocks, ordered / unordered /
**nested** lists, task lists (`- [x]` / `- [ ]`), `>` (and nested `>>`)
blockquotes, `---`/`***`/`___` rules, **tables** with column alignment,
images — a **local** image on its own line renders as truecolor half-block
(`▀`) art, remote/unreadable ones show a `🖼` placeholder — backslash escapes,
and links — inline `[text](url)`, reference-style `[text][ref]` / `[ref]` (with
`[ref]: url` definitions), `<url>`, and bare URLs.

Following a local-file link renders that page in place; `http(s)` URLs and
`#anchors` are reported in the status line rather than opened.

## How it works

`parse_markdown` (pure, unit-tested on the host) turns source lines into styled
segments; the WASM glue emits them via the render ABI
(`rtdvi_render_span` / `rtdvi_render_newline` / `rtdvi_render_open`). See
[`../../../doc/plugins.md`](../../../doc/plugins.md#render-buffers).
