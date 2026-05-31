# rtdvi.com — website

A tiny static site for [rtdvi](https://github.com/jroivas/rtdvi). It renders
the landing page (`content/index.md`) and the project's `../doc/*.md` into
static HTML using [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark).

This is a **standalone generator crate** — it is *not* part of the editor
build and adds no dependencies to it.

## Build

```sh
cd web
cargo run            # writes web/dist/
```

Or from the project root:

```sh
cargo run --manifest-path web/Cargo.toml
```

Then preview locally:

```sh
cd web/dist && python3 -m http.server 8000
# open http://localhost:8000
```

## Layout

```
web/
  content/index.md     landing page (Markdown)
  templates/page.html  shared HTML shell (logo, nav, footer)
  assets/
    logo.svg           aluminium 33cl can with "RTD" sideways
    style.css          minimal dark theme
  src/main.rs          the generator
  dist/                generated output (gitignored)
```

## What it does

- `content/index.md` → `dist/index.html`
- every `../doc/*.md` → `dist/docs/<name>.html`
- a generated `dist/docs/index.html` listing all docs
- local `*.md` links are rewritten to `*.html` so the docs cross-link
- `assets/` is copied to `dist/assets/`

## Deploy (GitHub Pages)

`dist/` is plain static files — publish it anywhere. For GitHub Pages,
point Pages at the generated `dist/` (e.g. via an Actions workflow that runs
the generator and uploads `web/dist/` as the Pages artifact), and set the
custom domain to `rtdvi.com`.
