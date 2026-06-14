//! Cross-line state machines for multi-line constructs (C `/* */` block
//! comments and Python triple-quoted strings).

use super::MultilineState;

// ---- Multi-line block comment tracking (C-style /* ... */) ----------------

/// Languages whose `/* … */` block comments can span multiple lines and so
/// need cross-line state threading (the single-line `/\*.*?\*/` rule only
/// catches comments that open and close on the same line).
pub(super) fn uses_c_block_comments(ft: &str) -> bool {
    matches!(
        ft,
        "c" | "cpp" | "rust" | "go" | "java" | "javascript" | "typescript" | "css"
    )
}

/// Advance block-comment `state` through one line, returning the state at
/// end-of-line. Skips `//` line comments and double-quoted strings so a
/// `/*` inside either doesn't spuriously open a comment.
pub(super) fn advance_c_block_state(line: &str, mut state: MultilineState) -> MultilineState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if state == MultilineState::BlockComment {
            if bytes.get(i..i + 2) == Some(b"*/") {
                state = MultilineState::None;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        // Outside a comment.
        if bytes.get(i..i + 2) == Some(b"//") {
            break; // rest of line is a line comment — nothing left to open
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            state = MultilineState::BlockComment;
            i += 2;
        } else if bytes[i] == b'"' {
            i = skip_dq_string(bytes, i);
        } else {
            i += 1;
        }
    }
    if state == MultilineState::BlockComment {
        MultilineState::BlockComment
    } else {
        MultilineState::None
    }
}

/// Byte offset of the `/*` that opens an *unterminated* block comment on
/// `line` (one with no matching `*/` before end-of-line), or `None`.
pub(super) fn find_c_block_open(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i..i + 2) == Some(b"//") {
            return None;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            let open = i;
            let mut j = i + 2;
            let mut closed = false;
            while j < bytes.len() {
                if bytes.get(j..j + 2) == Some(b"*/") {
                    closed = true;
                    j += 2;
                    break;
                }
                j += 1;
            }
            if !closed {
                return Some(open);
            }
            i = j;
        } else if bytes[i] == b'"' {
            i = skip_dq_string(bytes, i);
        } else {
            i += 1;
        }
    }
    None
}

/// Given `bytes[i] == b'"'`, return the index just past the closing quote
/// (honouring backslash escapes), or `bytes.len()` if unterminated.
fn skip_dq_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1; // skip opening quote
    while i < bytes.len() && bytes[i] != b'"' {
        if bytes[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    if i < bytes.len() {
        i += 1; // consume closing quote
    }
    i
}

// ---- Multi-line string tracking (Python triple-quoted strings) ------------

/// Advance `state` through one line, returning the state at the end of it.
pub(super) fn advance_python_state(line: &str, mut state: MultilineState) -> MultilineState {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match state {
            MultilineState::None => {
                if bytes[i] == b'#' {
                    break;
                }
                if bytes.get(i..i + 3) == Some(b"\"\"\"") {
                    state = MultilineState::TripleDouble;
                    i += 3;
                } else if bytes.get(i..i + 3) == Some(b"'''") {
                    state = MultilineState::TripleSingle;
                    i += 3;
                } else if bytes[i] == b'"' {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            MultilineState::TripleDouble => {
                if bytes.get(i..i + 3) == Some(b"\"\"\"") {
                    state = MultilineState::None;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            MultilineState::TripleSingle => {
                if bytes.get(i..i + 3) == Some(b"'''") {
                    state = MultilineState::None;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            // Python never enters block-comment state; treat defensively.
            MultilineState::BlockComment => i += 1,
        }
    }
    state
}

/// Find the byte position of the opening `delim` (`b"\"\"\""` or `b"'''"`)
/// in `line`, skipping over `#` comments. Returns `None` if not found.
pub(super) fn find_python_triple_open(line: &str, delim: &[u8]) -> Option<usize> {
    let bytes = line.as_bytes();
    let limit = bytes.len().saturating_sub(2);
    for i in 0..limit {
        if bytes[i] == b'#' {
            return None;
        }
        if bytes.get(i..i + 3) == Some(delim) {
            return Some(i);
        }
    }
    None
}
