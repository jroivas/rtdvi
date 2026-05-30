//! `]]` / `[[` use filetype-aware section rules: Rust item keywords at any
//! indent (so methods inside `impl` blocks become stops), Python `def`/
//! `class`, generic `{` at col 0 fallback.

use std::io::Write;

use rtdvi::keymap::keys::Key;
use rtdvi::{mode, Editor};
use tempfile::NamedTempFile;

fn type_keys(editor: &mut Editor, seq: &str) {
    for c in seq.chars() {
        mode::handle_key(editor, Key::char(c));
    }
}

fn open_with_extension(content: &str, suffix: &str) -> (Editor, NamedTempFile) {
    let mut f = NamedTempFile::with_suffix(suffix).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    (editor, f)
}

fn cursor_row(editor: &Editor) -> usize {
    editor.active_window().unwrap().cursor.row
}

// ---- Rust ------------------------------------------------------------------

/// The user's exact scenario: walking through their own buffer.rs.
const BUFFER_RS: &str = "\
//! The text buffer: a rope-backed editable document.
//!
//! line 3
//!

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ropey::{Rope, RopeSlice};
use thiserror::Error;

#[derive(Copy, Clone, Debug)]
pub struct BufferId(pub u32);

#[derive(Debug, Error)]
pub enum BufferError {
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub range: Range<usize>,
}

#[derive(Default)]
struct UndoStack {
    past: Vec<Vec<Edit>>,
}

pub struct Buffer {
    id: BufferId,
}

impl Buffer {
    pub fn scratch(id: BufferId) -> Self {
        Self {
            id,
        }
    }

    pub fn from_path(id: BufferId, path: &Path) -> Result<Self, ()> {
        let rope = if path.exists() {
            todo!()
        } else {
            todo!()
        };
        Ok(Self { id })
    }

    pub fn id(&self) -> BufferId {
        self.id
    }
}

fn apply_forward_to_rope(rope: &mut u32, edit: &u32) {
}
";

#[test]
fn double_close_walks_rust_top_level_items() {
    let (mut editor, _f) = open_with_extension(BUFFER_RS, ".rs");
    // Stops should be at item-declaration lines, not arbitrary `{`. Walk
    // from row 0 and collect the rows we land on.
    let mut visited = Vec::new();
    for _ in 0..15 {
        type_keys(&mut editor, "]]");
        visited.push(cursor_row(&editor));
    }
    // Each `]]` step should advance to a new section (or stop at EOF).
    // De-duplicate trailing repeats (when clamped at last line).
    while visited.len() > 1 && visited[visited.len() - 1] == visited[visited.len() - 2] {
        visited.pop();
    }
    // The lines that ARE item declarations in BUFFER_RS (0-indexed):
    // 5:  use std::fs;
    // 6:  use std::ops::Range;
    // 7:  use std::path::{Path, PathBuf};
    // 9:  use ropey::{...};
    // 10: use thiserror::Error;
    // 12: #[derive(...)]      (NOT — attribute)
    // 13: pub struct BufferId
    // 15: #[derive(Debug, Error)]  (NOT)
    // 16: pub enum BufferError
    // 19: #[derive(Clone...)]  (NOT)
    // 20: pub struct Edit
    // 24: #[derive(Default)]   (NOT)
    // 25: struct UndoStack
    // 29: pub struct Buffer
    // 33: impl Buffer
    // 34: pub fn scratch        (inside impl, indent 4)
    // 40: pub fn from_path      (inside impl)
    // 49: pub fn id             (inside impl)
    // 53: fn apply_forward_to_rope
    let expected = vec![5, 6, 7, 9, 10, 13, 16, 20, 25, 29, 33, 34, 40, 49, 54];
    assert_eq!(
        visited, expected,
        "section walk mismatch:\nexpected {:?}\ngot      {:?}",
        expected, visited
    );
}

#[test]
fn double_close_skips_if_and_let_blocks_inside_fn() {
    // Inside from_path() there's `let rope = if path.exists() { ... } else { ... };` —
    // jumping with `]]` must NOT stop on those inner braces.
    let (mut editor, _f) = open_with_extension(BUFFER_RS, ".rs");
    // `41G` → row 40 = `pub fn from_path`. From there `]]` should land on
    // `pub fn id` (row 49), not on the `let`/`if`/`else` blocks at 41-45.
    type_keys(&mut editor, "41G");
    assert_eq!(cursor_row(&editor), 40);
    type_keys(&mut editor, "]]");
    assert_eq!(cursor_row(&editor), 49);
}

#[test]
fn double_close_does_not_stop_on_struct_literal() {
    let (mut editor, _f) = open_with_extension(BUFFER_RS, ".rs");
    // `Self { ... }` inside scratch() is at row 35. From row 33 (impl), the
    // next stop must be 34 (`pub fn scratch`), then 40 (`pub fn from_path`),
    // never 35. `34G` → row 33.
    type_keys(&mut editor, "34G");
    assert_eq!(cursor_row(&editor), 33);
    type_keys(&mut editor, "]]");
    assert_eq!(cursor_row(&editor), 34);
    type_keys(&mut editor, "]]");
    assert_eq!(cursor_row(&editor), 40);
}

#[test]
fn double_open_walks_rust_sections_backward() {
    let (mut editor, _f) = open_with_extension(BUFFER_RS, ".rs");
    type_keys(&mut editor, "G"); // last line
    type_keys(&mut editor, "[[");
    assert_eq!(cursor_row(&editor), 54); // fn apply_forward_to_rope
    type_keys(&mut editor, "[[");
    assert_eq!(cursor_row(&editor), 49); // pub fn id
    type_keys(&mut editor, "[[");
    assert_eq!(cursor_row(&editor), 40); // pub fn from_path
}

// ---- Python ----------------------------------------------------------------

const PY_SRC: &str = "\
import sys

def hello():
    print('hi')

class Foo:
    def bar(self):
        if True:
            print(self)
        return 1

async def main():
    pass
";

#[test]
fn python_def_and_class_are_sections() {
    let (mut editor, _f) = open_with_extension(PY_SRC, ".py");
    let mut visited = Vec::new();
    for _ in 0..6 {
        type_keys(&mut editor, "]]");
        visited.push(cursor_row(&editor));
    }
    while visited.len() > 1 && visited[visited.len() - 1] == visited[visited.len() - 2] {
        visited.pop();
    }
    // def hello (row 2), class Foo (row 5), def bar (row 6), async def main
    // (row 11). The final entry is the EOF clamp (ropey adds a virtual
    // empty line after the trailing `\n`), so just check the prefix.
    assert!(
        visited.starts_with(&[2, 5, 6, 11]),
        "expected to start with [2,5,6,11], got {:?}",
        visited
    );
}

// ---- Generic ---------------------------------------------------------------

#[test]
fn generic_buffer_still_uses_curly_at_col_zero() {
    let content = "before\n{\n  a\n}\nmiddle\n{\n  b\n}\n";
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let mut editor = Editor::new();
    let id = editor.open_path(f.path()).unwrap();
    editor.focus_single(id);
    type_keys(&mut editor, "]]");
    assert_eq!(cursor_row(&editor), 1);
    type_keys(&mut editor, "]]");
    assert_eq!(cursor_row(&editor), 5);
}
