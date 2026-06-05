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
    let _guard = init_logging()?;
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

fn run<B: ratatui::backend::Backend>(
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
                        if editor.should_quit {
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

fn init_logging() -> Result<NonBlockingDropGuard> {
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("editor.log")?;
    let (writer, guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("RTDVI_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Ok(NonBlockingDropGuard { _writer: None, _guard: Some(guard) })
}

#[allow(dead_code)]
struct NonBlockingDropGuard {
    _writer: Option<NonBlocking>,
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}
