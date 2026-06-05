//! The text buffer: a rope-backed editable document.
//!
//! All mutations go through [`Buffer::insert`] / [`Buffer::delete`] /
//! [`Buffer::replace`]. Each returns an [`Edit`] value object describing
//! what changed, so the [`Editor`] can emit `BufferChanged` exactly once
//! per public mutation and feed the undo stack uniformly.
//!
//! Large files (> [`LARGE_FILE_THRESHOLD`]) are opened via `mmap` with a
//! lightweight line-start byte-offset index. Display reads directly from
//! the mapped region; the rope is built lazily on the first mutation.

use std::cell::RefCell;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

use memmap2::Mmap;
use ropey::{Rope, RopeSlice};
use thiserror::Error;

/// Files larger than this are opened in mmap mode (instant open, lazy rope).
const LARGE_FILE_THRESHOLD: u64 = 8 * 1024 * 1024; // 8 MiB

/// Bytes scanned synchronously at open time, covering the first viewport
/// before the background thread delivers its first batch.
const INIT_SCAN_BYTES: usize = 256 * 1024; // 256 KiB → ~2 600 lines at 100 chars/line

/// Bytes the background scan thread processes per batch before sending results.
const BG_BATCH_BYTES: usize = 1024 * 1024; // 1 MiB

// ── mmap state (owned by the main thread via RefCell) ────────────────────────

struct MmapState {
    line_starts: Vec<u32>,
    fully_indexed: bool,
    /// Batches of line-start offsets arriving from the background scan thread.
    /// `None` once the sender has been dropped (scan complete or buffer closed).
    rx: Option<mpsc::Receiver<Vec<u32>>>,
}

impl MmapState {
    /// Non-blocking drain: absorb any batches the background thread has ready.
    fn drain_pending(&mut self) {
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok(batch) => self.line_starts.extend_from_slice(&batch),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.fully_indexed = true;
                    self.rx = None;
                    break;
                }
            }
        }
    }

    /// Blocking drain: wait until the background thread finishes.
    fn wait_until_done(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        for batch in rx {
            self.line_starts.extend_from_slice(&batch);
        }
        self.fully_indexed = true;
    }

    fn line_count_lower(&self) -> usize {
        self.line_starts.len().max(1)
    }
}

// ── MmapBuffer ───────────────────────────────────────────────────────────────

/// Memory-mapped view of a large file with a background-built line index.
///
/// At construction a small sync scan covers the first viewport so the editor
/// renders immediately.  A background thread then scans the rest, sending
/// batches through a channel.  `line_count()` drains that channel on every
/// call, so by the time the user presses `G` the index is usually complete.
pub struct MmapBuffer {
    /// Shared with the background scan thread (read-only by both).
    mmap: Arc<Mmap>,
    /// Scan progress + channel receiver, protected by RefCell so `&self`
    /// display methods can drain the channel without needing `&mut Buffer`.
    state: RefCell<MmapState>,
}

impl MmapBuffer {
    fn new(mmap: Mmap) -> Self {
        let len = mmap.len();

        // ── synchronous boot scan ────────────────────────────────────────────
        // Scan the first INIT_SCAN_BYTES so the very first render has enough
        // lines without waiting for the background thread's first batch.
        let init_end = INIT_SCAN_BYTES.min(len);
        let mut line_starts = if len > 0 { vec![0u32] } else { vec![] };
        for pos in memchr::memchr_iter(b'\n', &mmap[..init_end]) {
            let next = pos + 1;
            if next < len {
                line_starts.push(next as u32);
            }
        }

        let mmap = Arc::new(mmap);
        let (rx, fully_indexed) = if init_end >= len {
            (None, true) // file fits entirely in the boot scan
        } else {
            let mmap2 = Arc::clone(&mmap);
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || scan_background(mmap2, init_end, tx));
            (Some(rx), false)
        };

        Self {
            mmap,
            state: RefCell::new(MmapState { line_starts, fully_indexed, rx }),
        }
    }

    fn line_string(&self, idx: usize) -> String {
        let state = self.state.borrow();
        let idx = idx.min(state.line_starts.len().saturating_sub(1));
        let start = state.line_starts[idx] as usize;
        let end = state
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.mmap.len() as u32) as usize;
        // Release the state borrow before touching the mmap slice.
        drop(state);
        let bytes = &self.mmap[start..end];
        let bytes = if bytes.ends_with(b"\r\n") {
            &bytes[..bytes.len() - 2]
        } else if bytes.ends_with(b"\n") {
            &bytes[..bytes.len() - 1]
        } else {
            bytes
        };
        String::from_utf8_lossy(bytes).into_owned()
    }
}

// ── background scan thread ────────────────────────────────────────────────────

fn scan_background(mmap: Arc<Mmap>, start_from: usize, tx: mpsc::Sender<Vec<u32>>) {
    let data: &[u8] = &mmap;
    let len = data.len();
    let mut pos = start_from;
    while pos < len {
        let end = (pos + BG_BATCH_BYTES).min(len);
        let batch: Vec<u32> = memchr::memchr_iter(b'\n', &data[pos..end])
            .filter_map(|i| {
                let next = pos + i + 1;
                if next < len { Some(next as u32) } else { None }
            })
            .collect();
        if tx.send(batch).is_err() {
            return; // buffer was closed, exit cleanly
        }
        pos = end;
    }
    // Dropping `tx` here signals the receiver that scanning is complete.
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(pub u32);

#[derive(Debug, Error)]
pub enum BufferError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("buffer has no associated path")]
    NoPath,
}

/// Description of a single mutation to a buffer.
///
/// The `inserted` text is the new content placed at `range.start`. The
/// `removed` text is what previously occupied `range`. Both are stored so
/// undo and event listeners can react without re-reading the rope.
#[derive(Clone, Debug)]
pub struct Edit {
    pub range: Range<usize>,
    pub removed: String,
    pub inserted: String,
}

/// Each entry on the undo stack is one *transaction*: a list of edits that
/// were applied in order, undone in reverse order. A single `insert` /
/// `delete` / `replace` call outside an explicit transaction is recorded
/// as a one-element entry, so undo behavior stays per-call for plain edits.
#[derive(Default)]
struct UndoStack {
    past: Vec<Vec<Edit>>,
    future: Vec<Vec<Edit>>,
    /// `Some` while a transaction is open; edits accumulate here instead of
    /// being pushed to `past` one-at-a-time.
    in_progress: Option<Vec<Edit>>,
}

pub struct Buffer {
    id: BufferId,
    rope: Rope,
    /// Set only for large files opened in mmap mode. `rope` is empty until
    /// [`Buffer::materialize`] is called; display reads from here instead.
    mmap_buf: Option<MmapBuffer>,
    path: Option<PathBuf>,
    /// Display name for scratch/virtual buffers that have no path.
    name: Option<String>,
    dirty: bool,
    undo: UndoStack,
    /// Manual syntax override set by `:set syntax=NAME` / `:set filetype=NAME`.
    /// When `Some`, takes precedence over auto-detection.
    syntax_override: Option<String>,
}

impl Buffer {
    pub fn scratch(id: BufferId) -> Self {
        Self {
            id,
            rope: Rope::new(),
            mmap_buf: None,
            path: None,
            name: None,
            dirty: false,
            undo: UndoStack::default(),
            syntax_override: None,
        }
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    /// Reset the dirty flag and clear the undo history. Used after
    /// programmatically populating a scratch buffer so it closes cleanly.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.undo = UndoStack::default();
    }

    pub fn from_path(id: BufferId, path: &Path) -> Result<Self, BufferError> {
        if !path.exists() {
            return Ok(Self {
                id,
                rope: Rope::new(),
                mmap_buf: None,
                path: Some(path.to_path_buf()),
                name: None,
                dirty: false,
                undo: UndoStack::default(),
                syntax_override: None,
            });
        }
        let file = fs::File::open(path)?;
        let size = file.metadata()?.len();
        if size >= LARGE_FILE_THRESHOLD {
            // SAFETY: standard mmap usage; we never mutate through this mapping.
            let mmap = unsafe { Mmap::map(&file)? };
            // Do NOT scan for newlines here — that's what made large files slow.
            // The index is built lazily in line_string() as lines are rendered.
            return Ok(Self {
                id,
                rope: Rope::new(),
                mmap_buf: Some(MmapBuffer::new(mmap)),
                path: Some(path.to_path_buf()),
                name: None,
                dirty: false,
                undo: UndoStack::default(),
                syntax_override: None,
            });
        }
        let rope = Rope::from_reader(std::io::BufReader::new(file))?;
        Ok(Self {
            id,
            rope,
            mmap_buf: None,
            path: Some(path.to_path_buf()),
            name: None,
            dirty: false,
            undo: UndoStack::default(),
            syntax_override: None,
        })
    }

    /// True while a large file is open but the rope has not yet been built.
    /// Display operations still work; editing / search trigger [`Self::materialize`].
    pub fn is_large_file_unloaded(&self) -> bool {
        self.mmap_buf.is_some()
    }

    /// Build the rope from the mmap data, then release the mmap.
    /// A no-op when the rope is already loaded. This is the only place where
    /// the rope-construction cost (~10 s for large files) is paid; it is
    /// deferred until the user first attempts to edit or search the file.
    pub fn materialize(&mut self) {
        let Some(mb) = self.mmap_buf.take() else {
            return;
        };
        // Drop the background channel so the scan thread exits promptly.
        drop(mb.state.into_inner().rx);
        self.rope =
            Rope::from_reader(std::io::Cursor::new(&mb.mmap[..])).unwrap_or_default();
    }

    /// Ensure the full line index is built so that [`Buffer::line_count`]
    /// returns the exact total. Needed before jumping to the last line (`G`).
    /// For small files (rope mode) this is a no-op.
    pub fn ensure_fully_indexed(&mut self) {
        if let Some(mb) = &mut self.mmap_buf {
            mb.state.get_mut().wait_until_done();
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, p: PathBuf) {
        self.path = Some(p);
    }

    /// Human-readable label for the status line. Returns the path when set,
    /// then the scratch name, then `[No Name]`.
    pub fn display_name(&self) -> String {
        if let Some(p) = &self.path {
            return p.display().to_string();
        }
        if let Some(n) = &self.name {
            return n.clone();
        }
        "[No Name]".to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn syntax_override(&self) -> Option<&str> {
        self.syntax_override.as_deref()
    }

    pub fn set_syntax_override(&mut self, ft: Option<String>) {
        self.syntax_override = ft;
    }

    /// Ensure at least `first + count` lines are indexed in mmap mode.
    /// Called once per render frame before the row loop so that
    /// `line_count()` returns an accurate answer for every row in the viewport.
    pub fn ensure_lines_visible(&self, _first: usize, _count: usize) {
        if let Some(mb) = &self.mmap_buf {
            mb.state.borrow_mut().drain_pending();
        }
    }

    /// Number of *displayed* lines in the buffer.
    ///
    /// For mmap buffers returns a lower bound that grows as the background scan
    /// delivers batches. Callers that need the exact total (e.g. `G`) must call
    /// [`Buffer::ensure_fully_indexed`] first.
    pub fn line_count(&self) -> usize {
        if let Some(mb) = &self.mmap_buf {
            mb.state.borrow_mut().drain_pending();
            return mb.state.borrow().line_count_lower();
        }
        let total = self.rope.len_lines();
        if self.ends_with_newline() {
            total.saturating_sub(1).max(1)
        } else {
            total.max(1)
        }
    }

    fn ends_with_newline(&self) -> bool {
        let n = self.rope.len_chars();
        n > 0 && self.rope.char(n - 1) == '\n'
    }

    pub fn len_chars(&self) -> usize {
        if let Some(mb) = &self.mmap_buf {
            // Use the byte length as an upper bound. char count ≤ byte count,
            // so this prevents cursor clamping to zero before the rope loads.
            return mb.mmap.len();
        }
        self.rope.len_chars()
    }

    pub fn line(&self, idx: usize) -> RopeSlice<'_> {
        self.rope.line(idx.min(self.rope.len_lines().saturating_sub(1)))
    }

    /// Line as a String with trailing newline trimmed, for display.
    ///
    /// For mmap buffers this also advances the lazy line index so that
    /// subsequent `line_count()` calls reflect at least `idx + 1` lines.
    pub fn line_string(&self, idx: usize) -> String {
        if let Some(mb) = &self.mmap_buf {
            return mb.line_string(idx);
        }
        let mut s: String = self.line(idx).to_string();
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    }

    pub fn char_to_line(&self, ch: usize) -> usize {
        self.rope.char_to_line(ch.min(self.rope.len_chars()))
    }

    pub fn line_to_char(&self, line: usize) -> usize {
        let cap = self.rope.len_lines().saturating_sub(1);
        self.rope.line_to_char(line.min(cap))
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn save(&mut self) -> Result<(), BufferError> {
        let path = self.path.clone().ok_or(BufferError::NoPath)?;
        self.save_as(&path)
    }

    /// Re-read the buffer's contents from its file on disk (`:e` / `:e!`),
    /// discarding any unsaved changes and the undo history. The path, display
    /// name, and manual syntax override are preserved. Errors if the buffer
    /// has no associated path.
    pub fn reload(&mut self) -> Result<(), BufferError> {
        let path = self.path.clone().ok_or(BufferError::NoPath)?;
        let fresh = Buffer::from_path(self.id, &path)?;
        self.rope = fresh.rope;
        self.mmap_buf = fresh.mmap_buf;
        self.dirty = false;
        self.undo = UndoStack::default();
        Ok(())
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), BufferError> {
        use std::io::Write;
        let mut file = std::io::BufWriter::new(fs::File::create(path)?);
        if let Some(mb) = &self.mmap_buf {
            file.write_all(&mb.mmap[..])?;
        } else {
            self.rope.write_to(&mut file)?;
        }
        file.flush()?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn insert(&mut self, ch: usize, text: &str) -> Edit {
        self.materialize();
        let ch = ch.min(self.rope.len_chars());
        self.rope.insert(ch, text);
        let edit = Edit {
            range: ch..ch + text.chars().count(),
            removed: String::new(),
            inserted: text.to_string(),
        };
        self.dirty = true;
        self.record_edit(edit.clone());
        edit
    }

    pub fn delete(&mut self, range: Range<usize>) -> Edit {
        self.materialize();
        let start = range.start.min(self.rope.len_chars());
        let end = range.end.min(self.rope.len_chars()).max(start);
        let removed: String = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        let edit = Edit {
            range: start..start,
            removed,
            inserted: String::new(),
        };
        self.dirty = true;
        self.record_edit(edit.clone());
        edit
    }

    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Edit {
        self.materialize();
        let start = range.start.min(self.rope.len_chars());
        let end = range.end.min(self.rope.len_chars()).max(start);
        let removed: String = self.rope.slice(start..end).to_string();
        self.rope.remove(start..end);
        self.rope.insert(start, text);
        let edit = Edit {
            range: start..start + text.chars().count(),
            removed,
            inserted: text.to_string(),
        };
        self.dirty = true;
        self.record_edit(edit.clone());
        edit
    }

    /// Open a new undo transaction. Subsequent mutations are coalesced into
    /// a single undo step until [`Buffer::end_transaction`] is called.
    /// If a transaction is already open, it is committed first to avoid losing edits.
    pub fn begin_transaction(&mut self) {
        self.commit_transaction();
        self.undo.in_progress = Some(Vec::new());
    }

    /// Close the current transaction, pushing it as a single undo entry.
    /// No-op if no transaction is open.
    pub fn end_transaction(&mut self) {
        self.commit_transaction();
    }

    pub fn in_transaction(&self) -> bool {
        self.undo.in_progress.is_some()
    }

    fn commit_transaction(&mut self) {
        if let Some(edits) = self.undo.in_progress.take() {
            if !edits.is_empty() {
                self.undo.past.push(edits);
                self.undo.future.clear();
            }
        }
    }

    fn record_edit(&mut self, edit: Edit) {
        match &mut self.undo.in_progress {
            Some(buf) => buf.push(edit),
            None => {
                self.undo.past.push(vec![edit]);
                self.undo.future.clear();
            }
        }
    }

    /// Undo one transaction. Returns a synthetic `Edit` pointing at the
    /// start of the FIRST edit in the transaction, so callers can place the
    /// cursor at the natural "top" of the change.
    pub fn undo(&mut self) -> Option<Edit> {
        // If a transaction is somehow open, commit it first so it ends up
        // as a single, complete unit on the stack — but never undo it as
        // part of this call; that would surprise the user.
        self.commit_transaction();
        let edits = self.undo.past.pop()?;
        for edit in edits.iter().rev() {
            apply_inverse_to_rope(&mut self.rope, edit);
        }
        let result = edits.first().map(|e| Edit {
            range: e.range.start..e.range.start,
            removed: String::new(),
            inserted: e.removed.clone(),
        });
        self.undo.future.push(edits);
        self.dirty = !self.undo.past.is_empty() || self.undo.in_progress.is_some();
        result
    }

    pub fn redo(&mut self) -> Option<Edit> {
        self.commit_transaction();
        let edits = self.undo.future.pop()?;
        for edit in &edits {
            apply_forward_to_rope(&mut self.rope, edit);
        }
        let result = edits.first().cloned();
        self.undo.past.push(edits);
        self.dirty = true;
        result
    }
}

fn apply_forward_to_rope(rope: &mut Rope, edit: &Edit) {
    let removed_chars = edit.removed.chars().count();
    let start = edit.range.start;
    rope.remove(start..start + removed_chars);
    rope.insert(start, &edit.inserted);
}

fn apply_inverse_to_rope(rope: &mut Rope, edit: &Edit) {
    let inserted_chars = edit.inserted.chars().count();
    let start = edit.range.start;
    rope.remove(start..start + inserted_chars);
    rope.insert(start, &edit.removed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        let mut b = Buffer::scratch(BufferId(0));
        b.insert(0, s);
        // Reset dirty flag since this is "initial content"
        b.undo = UndoStack::default();
        b.dirty = false;
        b
    }

    #[test]
    fn insert_and_line_indexing() {
        let mut b = buf("hello\nworld\n");
        // `line_count` is the *displayed* count: trailing `\n` is treated
        // as a line terminator, not as a separate empty line.
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line_string(0), "hello");
        assert_eq!(b.line_string(1), "world");
        b.insert(5, "!");
        assert_eq!(b.line_string(0), "hello!");
    }

    #[test]
    fn line_count_without_trailing_newline() {
        let b = buf("a\nb");
        assert_eq!(b.line_count(), 2);
    }

    #[test]
    fn line_count_with_trailing_newline_drops_virtual_line() {
        let b = buf("a\nb\n");
        assert_eq!(b.line_count(), 2);
    }

    #[test]
    fn empty_buffer_has_one_line() {
        let b = buf("");
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn double_trailing_newline_keeps_the_blank() {
        // The user has intentionally added a blank line at end; it stays.
        let b = buf("a\n\n");
        assert_eq!(b.line_count(), 2);
    }

    #[test]
    fn delete_round_trip() {
        let mut b = buf("abcdef");
        b.delete(1..4);
        assert_eq!(b.rope.to_string(), "aef");
        b.undo();
        assert_eq!(b.rope.to_string(), "abcdef");
        b.redo();
        assert_eq!(b.rope.to_string(), "aef");
    }

    #[test]
    fn replace_round_trip() {
        let mut b = buf("hello world");
        b.replace(6..11, "rust!");
        assert_eq!(b.rope.to_string(), "hello rust!");
        b.undo();
        assert_eq!(b.rope.to_string(), "hello world");
    }

    #[test]
    fn multi_byte_round_trip() {
        let mut b = buf("a漢b");
        // chars: a(0), 漢(1), b(2). delete the 漢:
        b.delete(1..2);
        assert_eq!(b.rope.to_string(), "ab");
        b.undo();
        assert_eq!(b.rope.to_string(), "a漢b");
    }
}
