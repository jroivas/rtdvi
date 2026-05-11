use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event as XEvent};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tracing::info;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::EnvFilter;

use jvim::keymap::keys::from_crossterm;
use jvim::{mode, ui, Editor};

#[derive(Parser, Debug)]
#[command(name = "jvim", about = "small modal text editor")]
struct Cli {
    /// File to open. Omit for a scratch buffer.
    file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _guard = init_logging()?;
    info!("starting jvim");

    let mut editor = Editor::new();
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
    while !editor.should_quit {
        terminal.draw(|f| ui::render(editor, f)).map(|_| ())?;
        // Poll so a Ctrl-C / SIGWINCH that comes through as an event can
        // wake the loop promptly.
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                XEvent::Key(k) => {
                    if let Some(key) = from_crossterm(k) {
                        // Clear any leftover status from previous tick.
                        editor.status_message = None;
                        mode::handle_key(editor, key);
                    }
                }
                XEvent::Resize(_, _) => {
                    // ratatui handles the size on the next draw.
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn teardown_terminal<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<()> {
    disable_raw_mode()?;
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
        .with_env_filter(EnvFilter::try_from_env("JVIM_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
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
