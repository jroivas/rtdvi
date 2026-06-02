//! Embedded terminal: a PTY running a child process (a shell by default)
//! whose output is parsed by [`vt100`] into a screen grid for rendering.
//!
//! One [`Terminal`] is created per `:term` invocation and stored in the
//! editor keyed by the [`BufferId`] of the scratch buffer that backs its
//! window. A background thread drains the PTY master and feeds the parser;
//! the main thread reads the parsed grid on every render and forwards
//! keystrokes back to the child while the terminal window has focus.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Rows reserved for vt100 scrollback. Keeps a little history so a fast
/// `cat` of a long file isn't instantly lost, without unbounded growth.
const SCROLLBACK: usize = 1000;

pub struct Terminal {
    /// Shared with the reader thread, which feeds it raw PTY bytes.
    pub parser: Arc<Mutex<vt100::Parser>>,
    /// Master-side writer: keystrokes go here to reach the child.
    writer: Box<dyn Write + Send>,
    /// Master handle, kept so the PTY can be resized when the window changes.
    master: Box<dyn MasterPty + Send>,
    /// The spawned child (shell). Held so it can be reaped / killed.
    child: Box<dyn Child + Send + Sync>,
    /// Set by the reader thread when the PTY hits EOF (child exited).
    exited: Arc<AtomicBool>,
    /// Toggled whenever new output arrives so the main loop knows to redraw.
    dirty: Arc<AtomicBool>,
    /// Current grid size, to avoid redundant resizes.
    rows: u16,
    cols: u16,
    /// Display label (the command line), shown in the statusline.
    pub command: String,
}

impl Terminal {
    /// Spawn `command` (or the user's `$SHELL`, falling back to `/bin/sh`,
    /// when `command` is empty) on a fresh PTY sized `cols`×`rows`.
    pub fn spawn(
        cols: u16,
        rows: u16,
        command: &str,
        cwd: Option<&Path>,
    ) -> Result<Self, String> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;

        let (mut cmd, label) = if command.trim().is_empty() {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            (CommandBuilder::new(&shell), shell)
        } else {
            // Run through the shell so pipes / args / globs behave like `:!`.
            let mut c = CommandBuilder::new("sh");
            c.arg("-c");
            c.arg(command);
            (c, command.to_string())
        };
        // Inherit our environment so PATH etc. work, then announce a sane TERM.
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // Drop the slave once the child owns it, so EOF is delivered to the
        // master reader when the child exits.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let exited = Arc::new(AtomicBool::new(false));
        let dirty = Arc::new(AtomicBool::new(true));

        {
            let parser = Arc::clone(&parser);
            let exited = Arc::clone(&exited);
            let dirty = Arc::clone(&dirty);
            let mut reader = reader;
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            exited.store(true, Ordering::SeqCst);
                            dirty.store(true, Ordering::SeqCst);
                            break;
                        }
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            dirty.store(true, Ordering::SeqCst);
                        }
                    }
                }
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            exited,
            dirty,
            rows,
            cols,
            command: label,
        })
    }

    /// Resize the grid and the underlying PTY to `cols`×`rows`. No-op when
    /// the size is unchanged.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.dirty.store(true, Ordering::SeqCst);
    }

    /// Forward raw bytes to the child process.
    pub fn send(&mut self, bytes: &[u8]) {
        if self.exited.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// True once the child process has exited (PTY EOF).
    pub fn is_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Read and clear the redraw-needed flag.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::SeqCst)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Kill the child if it's still alive, then reap it so we don't leave
        // a zombie behind when a terminal window is closed with `<C-w>c`.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
