use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event as XEvent,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tracing::info;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::EnvFilter;

use rtdvi::keymap::keys::from_crossterm;
use rtdvi::{mode, ui, Editor};

#[derive(Parser, Debug)]
#[command(name = "rtdvi", about = "small modal text editor")]
struct Cli {
    /// File to open. Omit for a scratch buffer.
    file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _guard = init_logging();
    info!("starting rtdvi");

    let mut editor = Editor::new();
    if let Some((cfg_path, _fmt)) = rtdvi::config::loader::find_existing() {
        match rtdvi::config::loader::load_or_default(&cfg_path) {
            Ok(cfg) => {
                editor.apply_config(cfg);
                editor.config_path = Some(cfg_path);
            }
            Err(e) => tracing::warn!("config load failed: {e}"),
        }
    }
    // Default colorscheme. Try a few in order; fall back to vim's built-in
    // SynColor defaults if none of the named schemes are present.
    for candidate in ["default", "desert"] {
        match rtdvi::colorscheme::load(candidate) {
            Ok(scheme) => {
                editor.colorscheme = scheme;
                break;
            }
            Err(e) => tracing::info!("colorscheme {candidate}: {e}"),
        }
    }
    editor.history.entries = rtdvi::history::load_entries(&editor.history.file_path);
    editor.search_history.entries =
        rtdvi::history::load_entries(&editor.search_history.file_path);

    let buf_id = match cli.file {
        Some(p) => editor.open_path(&p).with_context(|| format!("opening {:?}", p))?,
        None => editor.open_scratch(),
    };
    editor.focus_single(buf_id);

    let mut terminal = setup_terminal()?;
    let result = run(&mut editor, &mut terminal);
    teardown_terminal(&mut terminal)?;
    result
}

fn run<B: ratatui::backend::Backend + std::io::Write>(
    editor: &mut Editor,
    terminal: &mut Terminal<B>,
) -> Result<()> {
    // Cap the work done in a single batch so an unusually long burst of
    // events can't starve the renderer indefinitely. In normal use we
    // never approach this — auto-repeat usually queues a few dozen keys
    // at most before there's a natural pause.
    const MAX_EVENTS_PER_FRAME: usize = 256;

    while !editor.should_quit {
        // Drive background plugin loading: poll for finished compilations and
        // start the next pending entry. Non-blocking — returns immediately if
        // nothing is ready.
        #[cfg(feature = "plugins")]
        rtdvi::plugin::tick(editor);

        editor.lsp_poll();
        // Close any terminal whose job has exited (shell `exit`), then drop
        // back to Normal mode if that left a non-terminal window focused.
        if editor.reap_terminals() {
            rtdvi::mode::sync_mode_for_active(editor);
        }
        terminal.draw(|f| ui::render(editor, f)).map(|_| ())?;

        // A live terminal produces output asynchronously, so poll briefly to
        // keep its display fresh. Otherwise idle until the next keypress.
        let term_ms = if editor.has_live_terminal() { 16 } else { 250 };
        // While plugins are loading, wake up frequently to catch completions.
        #[cfg(feature = "plugins")]
        let poll_ms = if editor.plugins.is_loading() { 50 } else { term_ms };
        #[cfg(not(feature = "plugins"))]
        let poll_ms = term_ms;
        if !event::poll(Duration::from_millis(poll_ms))? {
            continue;
        }
        // Drain every event that's already pending in the queue before
        // re-rendering. Holding `j` typically queues dozens of events;
        // processing them all and rendering once at the end keeps the
        // editor responsive instead of one-render-per-keystroke.
        let mut processed = 0;
        loop {
            match event::read()? {
                XEvent::Key(k) => {
                    if let Some(key) = from_crossterm(k) {
                        editor.status_message = None;
                        mode::handle_key(editor, key);
                        // `:sh` and `:q` both need us out of the event-drain
                        // loop: the shell must own the terminal, and quitting
                        // shouldn't process further queued keys.
                        if editor.should_quit || editor.pending_shell {
                            break;
                        }
                    }
                }
                XEvent::Paste(text) => {
                    // Bracketed paste: insert the whole chunk verbatim,
                    // bypassing per-keystroke autoindent regardless of the
                    // `:paste` setting.
                    editor.status_message = None;
                    mode::handle_paste(editor, &text);
                }
                XEvent::Resize(_, _) => {
                    // ratatui re-reads the size on the next draw — nothing
                    // to do here, but consume the event so it doesn't
                    // delay subsequent reads.
                }
                _ => {}
            }
            processed += 1;
            if processed >= MAX_EVENTS_PER_FRAME {
                break;
            }
            // Stop draining the moment the queue is empty so the next
            // render reflects whatever state we just landed in.
            if !event::poll(Duration::from_millis(0))? {
                break;
            }
        }

        // `:sh` — suspend the TUI, run an interactive shell, then restore.
        // Done here (not inside the ex command) because only the render loop
        // owns the terminal handle.
        if std::mem::take(&mut editor.pending_shell) {
            suspend_and_run_shell(terminal)?;
        }
    }
    Ok(())
}

/// Leave the alternate screen and cooked-mode the terminal so a child shell
/// owns it, run `$SHELL` (falling back to `/bin/sh`) to completion, then
/// re-enter the TUI and force a full redraw. Mirrors vim's `:sh`.
fn suspend_and_run_shell<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    // Hand the terminal to the shell.
    disable_raw_mode()?;
    terminal.backend_mut().execute(DisableBracketedPaste)?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    // Inherits our stdio, so the shell is fully interactive.
    let status = std::process::Command::new(&shell).status();

    // Take the terminal back and repaint from scratch — the shell scribbled
    // over the normal screen and ratatui's back-buffer is now stale.
    enable_raw_mode()?;
    terminal.backend_mut().execute(EnterAlternateScreen)?;
    terminal.backend_mut().execute(EnableBracketedPaste)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    if let Err(e) = status {
        tracing::warn!("sh: failed to launch {shell:?}: {e}");
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    // Bracketed paste: the terminal wraps pasted text so we receive it as a
    // single `Event::Paste`, letting us insert it verbatim (no per-char
    // autoindent) without the user toggling `:paste`.
    stdout.execute(EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn teardown_terminal<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(DisableBracketedPaste)?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Set up file logging — but only when explicitly opted into via `RTDVI_LOG`.
///
/// By default nothing is written: no file is created (keeping the workspace
/// clean and letting the editor run on read-only media). When `RTDVI_LOG` is
/// set it doubles as the env-filter directive (e.g. `RTDVI_LOG=debug`),
/// defaulting to `error` if it isn't a valid filter. The log goes to the file
/// named by `RTDVI_LOG_PATH`, or to a per-process file under `~/.log/rtdvi/`
/// when that's unset. Any failure along the way silently disables logging
/// rather than aborting startup.
fn init_logging() -> NonBlockingDropGuard {
    let Ok(filter_str) = std::env::var("RTDVI_LOG") else {
        return NonBlockingDropGuard::default();
    };
    if filter_str.trim().is_empty() {
        return NonBlockingDropGuard::default();
    }
    let Some(path) = log_file_path() else {
        return NonBlockingDropGuard::default();
    };
    // Create the parent directory if we're choosing the path ourselves or the
    // user pointed at a nested file. Failure → disable logging, don't abort.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && std::fs::create_dir_all(parent).is_err() {
            return NonBlockingDropGuard::default();
        }
    }
    let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return NonBlockingDropGuard::default();
    };
    let (writer, guard) = tracing_appender::non_blocking(log_file);
    // `RTDVI_LOG` is the filter directive; fall back to `error` (not `info`)
    // when it isn't a valid one.
    let filter = EnvFilter::try_new(&filter_str).unwrap_or_else(|_| EnvFilter::new("error"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    NonBlockingDropGuard { _writer: None, _guard: Some(guard) }
}

/// The log file to write to: the exact path in `$RTDVI_LOG_PATH` if set,
/// otherwise a per-process file `~/.log/rtdvi/rtdvi-<unix-seconds>-<pid>.log`.
/// The per-process name keeps concurrent editors from sharing one file when
/// using the default location. Returns `None` (disabling logging) when no
/// path can be resolved.
fn log_file_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RTDVI_LOG_PATH") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_name = format!("rtdvi-{secs}-{}.log", std::process::id());
    Some(PathBuf::from(home).join(".log").join("rtdvi").join(file_name))
}

#[allow(dead_code)]
#[derive(Default)]
struct NonBlockingDropGuard {
    _writer: Option<NonBlocking>,
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}
