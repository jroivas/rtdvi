//! Vim-style jumplist.
//!
//! Each "jump" (`gd`, `gg`, `*`, `/`, `]]`, …) appends the position the
//! cursor was AT before the jump. `<C-o>` walks back through that history;
//! `<C-i>` / `<Tab>` walks forward. After going back and then performing
//! a fresh jump, the forward history is discarded — same as vim.

use crate::buffer::BufferId;

/// One historical cursor position.
#[derive(Debug, Clone, Copy)]
pub struct JumpEntry {
    pub buffer: BufferId,
    pub row: usize,
    pub col: usize,
}

/// Maximum number of entries we keep. Vim defaults to 100; we mirror that.
pub const MAX_ENTRIES: usize = 100;

#[derive(Debug, Default)]
pub struct Jumplist {
    /// Chronological list of past positions.
    entries: Vec<JumpEntry>,
    /// "Cursor into the list":
    /// - `entries.len()` → at the live position (most recent, not stored).
    /// - `i < entries.len()` → currently AT `entries[i]`, came here via
    ///   `<C-o>`. Forward entries (`entries[i+1..]`) are the future-stack.
    pointer: usize,
}

impl Jumplist {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[JumpEntry] {
        &self.entries
    }
    pub fn pointer(&self) -> usize {
        self.pointer
    }

    /// Record that the cursor is about to leave `from` for some other
    /// position. Forward history is discarded if we'd been part-way back.
    pub fn record(&mut self, from: JumpEntry) {
        if self.pointer < self.entries.len() {
            self.entries.truncate(self.pointer);
        }
        // Coalesce immediate duplicates so a flurry of jumps from the same
        // line doesn't clutter the list.
        if let Some(last) = self.entries.last() {
            if last.buffer == from.buffer && last.row == from.row {
                self.pointer = self.entries.len();
                return;
            }
        }
        self.entries.push(from);
        if self.entries.len() > MAX_ENTRIES {
            let drop = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(0..drop);
        }
        self.pointer = self.entries.len();
    }

    /// `<C-o>`: walk back one step. `current` is the position we're at right
    /// now — stashed in the list (so `<C-i>` can return) only the first
    /// time we step back from the live position.
    pub fn back(&mut self, current: JumpEntry) -> Option<JumpEntry> {
        if self.pointer == 0 {
            return None;
        }
        if self.pointer == self.entries.len() {
            // First step back: remember where we are now.
            self.entries.push(current);
            // Don't bump pointer — we want to consume the entry below.
        }
        self.pointer -= 1;
        self.entries.get(self.pointer).copied()
    }

    /// `<C-i>` / `<Tab>`: walk forward one step.
    pub fn forward(&mut self) -> Option<JumpEntry> {
        if self.pointer + 1 >= self.entries.len() {
            return None;
        }
        self.pointer += 1;
        self.entries.get(self.pointer).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(b: u32, r: usize, c: usize) -> JumpEntry {
        JumpEntry {
            buffer: BufferId(b),
            row: r,
            col: c,
        }
    }

    #[test]
    fn back_then_forward_round_trips() {
        let mut jl = Jumplist::new();
        // Started at A. gd → B. record A.
        jl.record(e(0, 0, 0));
        // gd → C. record B.
        jl.record(e(0, 10, 0));
        // <C-o> from C.
        let p = jl.back(e(0, 20, 0)).unwrap();
        assert_eq!((p.row, p.col), (10, 0));
        // <C-o> again.
        let p = jl.back(e(0, 10, 0)).unwrap();
        assert_eq!((p.row, p.col), (0, 0));
        // <C-i> back.
        let p = jl.forward().unwrap();
        assert_eq!((p.row, p.col), (10, 0));
        // <C-i> to live.
        let p = jl.forward().unwrap();
        assert_eq!((p.row, p.col), (20, 0));
    }

    #[test]
    fn fresh_jump_drops_forward_history() {
        let mut jl = Jumplist::new();
        jl.record(e(0, 0, 0)); // gd → B
        jl.record(e(0, 10, 0)); // gd → C
        jl.back(e(0, 20, 0)); // back to B
        jl.back(e(0, 10, 0)); // back to A
        // Now do a fresh jump (was at A, going to D).
        jl.record(e(0, 0, 0));
        // Forward history (C, B-stash) gone.
        // <C-i> should yield nothing now since we're at the live end.
        assert!(jl.forward().is_none());
        // But <C-o> still gives us the A we just pushed.
        let p = jl.back(e(0, 50, 0)).unwrap();
        assert_eq!(p.row, 0);
    }

    #[test]
    fn back_at_empty_is_noop() {
        let mut jl = Jumplist::new();
        assert!(jl.back(e(0, 0, 0)).is_none());
    }

    #[test]
    fn duplicates_on_same_line_coalesce() {
        let mut jl = Jumplist::new();
        jl.record(e(0, 5, 0));
        jl.record(e(0, 5, 4)); // same line — should not append
        assert_eq!(jl.entries().len(), 1);
    }

    #[test]
    fn list_caps_at_max() {
        let mut jl = Jumplist::new();
        for i in 0..(MAX_ENTRIES + 30) {
            jl.record(e(0, i, 0));
        }
        assert_eq!(jl.entries().len(), MAX_ENTRIES);
    }
}
