//! The text buffer: a rope-backed editable document.
//!
//! All mutations go through [`Buffer::insert`] / [`Buffer::delete`] /
//! [`Buffer::replace`]. Each returns an [`Edit`] value object describing
//! what changed, so the [`Editor`] can emit `BufferChanged` exactly once
//! per public mutation and feed the undo stack uniformly.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ropey::{Rope, RopeSlice};
use thiserror::Error;

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

#[derive(Default)]
struct UndoStack {
    past: Vec<Edit>,
    future: Vec<Edit>,
}

pub struct Buffer {
    id: BufferId,
    rope: Rope,
    path: Option<PathBuf>,
    dirty: bool,
    undo: UndoStack,
}

impl Buffer {
    pub fn scratch(id: BufferId) -> Self {
        Self {
            id,
            rope: Rope::new(),
            path: None,
            dirty: false,
            undo: UndoStack::default(),
        }
    }

    pub fn from_path(id: BufferId, path: &Path) -> Result<Self, BufferError> {
        let rope = if path.exists() {
            Rope::from_reader(std::io::BufReader::new(fs::File::open(path)?))?
        } else {
            Rope::new()
        };
        Ok(Self {
            id,
            rope,
            path: Some(path.to_path_buf()),
            dirty: false,
            undo: UndoStack::default(),
        })
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

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line(&self, idx: usize) -> RopeSlice<'_> {
        self.rope.line(idx.min(self.rope.len_lines().saturating_sub(1)))
    }

    /// Line as a String with trailing newline trimmed, for display.
    pub fn line_string(&self, idx: usize) -> String {
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

    pub fn save_as(&mut self, path: &Path) -> Result<(), BufferError> {
        let mut file = std::io::BufWriter::new(fs::File::create(path)?);
        self.rope.write_to(&mut file)?;
        use std::io::Write;
        file.flush()?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn insert(&mut self, ch: usize, text: &str) -> Edit {
        let ch = ch.min(self.rope.len_chars());
        self.rope.insert(ch, text);
        let edit = Edit {
            range: ch..ch + text.chars().count(),
            removed: String::new(),
            inserted: text.to_string(),
        };
        self.dirty = true;
        self.undo.past.push(edit.clone());
        self.undo.future.clear();
        edit
    }

    pub fn delete(&mut self, range: Range<usize>) -> Edit {
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
        self.undo.past.push(edit.clone());
        self.undo.future.clear();
        edit
    }

    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Edit {
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
        self.undo.past.push(edit.clone());
        self.undo.future.clear();
        edit
    }

    /// Undo the last edit. Returns the *inverse* edit that was applied.
    pub fn undo(&mut self) -> Option<Edit> {
        let last = self.undo.past.pop()?;
        let inverse = apply_inverse(&mut self.rope, &last);
        self.undo.future.push(last);
        self.dirty = !self.undo.past.is_empty();
        Some(inverse)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        let next = self.undo.future.pop()?;
        apply_forward(&mut self.rope, &next);
        self.undo.past.push(next.clone());
        self.dirty = true;
        Some(next)
    }
}

fn apply_forward(rope: &mut Rope, edit: &Edit) {
    // `edit.range.start` is the anchor; remove `removed.len_chars()` then insert `inserted`.
    let removed_chars = edit.removed.chars().count();
    let start = edit.range.start;
    rope.remove(start..start + removed_chars);
    rope.insert(start, &edit.inserted);
}

fn apply_inverse(rope: &mut Rope, edit: &Edit) -> Edit {
    let inserted_chars = edit.inserted.chars().count();
    let start = edit.range.start;
    rope.remove(start..start + inserted_chars);
    rope.insert(start, &edit.removed);
    Edit {
        range: start..start + edit.removed.chars().count(),
        removed: edit.inserted.clone(),
        inserted: edit.removed.clone(),
    }
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
        assert_eq!(b.line_count(), 3); // ropey: trailing newline -> empty line after
        assert_eq!(b.line_string(0), "hello");
        assert_eq!(b.line_string(1), "world");
        b.insert(5, "!");
        assert_eq!(b.line_string(0), "hello!");
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
