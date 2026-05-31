# rtdvi

<div class="hero">
  <img src="assets/logo.svg" alt="rtdvi can logo">
  <div>
    <p class="tagline">A small, <strong>heavily opinionated</strong> vi clone.</p>
    <p>Modal editing in the terminal — familiar keys, a tight feature set, and
    sensible defaults baked in rather than configured to death.</p>
  </div>
</div>

rtdvi (*"ready-to-drink vi"*) is a from-scratch modal text editor written in
Rust. It speaks vi: normal / insert / visual modes, operators and motions,
`:` commands, registers, marks, splits and tabs. It is **not** a vim
configuration framework and it is not trying to be Neovim — it picks
defaults and sticks with them.

## Opinionated by design

- **Vi muscle memory, curated.** The motions, operators, and `:` commands
  you reach for, without the long tail of obscure compatibility behaviour.
- **Batteries included, not bolted on.** Native LSP client, syntax
  highlighting, fuzzy file finder, autoindent, system-clipboard integration,
  and bracketed paste all ship in the box.
- **Sensible defaults over endless options.** Things work well out of the
  box; the config exists to nudge, not to assemble the editor from parts.
- **Small and fast.** A focused Rust core, a single static binary, instant
  start-up.

## A taste

```sh
rtdvi src/main.rs      # auto-detects filetype, loads syntax + LSP
rtdvi                  # scratch buffer
```

- `gq` reflows comments to your text width, or formats code via the LSP.
- `gd` / `gr` / `K` jump to definitions, list references, show hover.
- `:ff` fuzzy-finds files; `<C-w>` family drives splits.
- `:paste` / bracketed paste insert verbatim — no cascading autoindent.

## Get it

```sh
git clone https://github.com/jroivas/rtdvi
cd rtdvi
cargo build --release
cargo install --path .          # installs `rtdvi`
# optional short command:
make rvi-alias                  # symlinks `rvi` -> rtdvi
```

See the [documentation](docs/index.html) for the full picture —
[configuration](docs/configuration.html), [modes](docs/modes.html),
[LSP](docs/lsp.html), [plugins](docs/plugins.html), and more.

## Links

- [Documentation](docs/index.html)
- [Source on GitHub](https://github.com/jroivas/rtdvi)
