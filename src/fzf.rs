//! Native fuzzy file finder.
//!
//! `Index` holds the (relative-path) file list for a workspace, built
//! once on first use and reused on every subsequent query. `search`
//! runs each whitespace-delimited token of the query through the Skim
//! fuzzy matcher independently — *all* tokens must match — and sums the
//! per-token scores. The top N results are returned ranked by score.
//!
//! The classic fzf example: `:fzf posix.c share` ranks
//! `lib/vfs/access_layer/smb/smb_share_vfs_posix.c` near the top
//! because both `posix.c` and `share` fuzzy-match against the path with
//! reasonable density.

use std::path::{Path, PathBuf};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ignore::WalkBuilder;

/// Cap on the number of files we'll index. Large monorepos otherwise
/// chew CPU on every query. Tune via [`MAX_FILES`].
pub const MAX_FILES: usize = 100_000;

/// Cap on results returned per query — the popup shows ~10 at a time so
/// 200 is plenty to scroll through.
pub const RESULT_LIMIT: usize = 200;

#[derive(Debug, Default)]
pub struct Index {
    pub root: PathBuf,
    /// Relative paths from `root`, sorted by length then lexicographic
    /// order so empty-query browsing produces a stable list.
    pub files: Vec<String>,
}

impl Index {
    /// Walk `root`, honouring `.gitignore` / `.git/info/exclude` /
    /// global excludes via the `ignore` crate. Hidden files are
    /// skipped (vim-ish default).
    pub fn build(root: &Path) -> Self {
        let mut files: Vec<String> = Vec::new();
        let walker = WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .max_filesize(Some(50 * 1024 * 1024)) // skip huge binaries
            .build();
        for entry in walker.flatten() {
            if !entry.file_type().map_or(false, |t| t.is_file()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            if let Some(s) = rel.to_str() {
                files.push(s.to_string());
                if files.len() >= MAX_FILES {
                    break;
                }
            }
        }
        files.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
        Self {
            root: root.to_path_buf(),
            files,
        }
    }
}

/// Search the index. Returns `(path, score)` pairs sorted by descending
/// score, capped at [`RESULT_LIMIT`]. Empty queries return the first
/// chunk of the index so the popup isn't empty.
pub fn search(index: &Index, query: &str) -> Vec<(String, i64)> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return index
            .files
            .iter()
            .take(RESULT_LIMIT)
            .map(|s| (s.clone(), 0))
            .collect();
    }
    let matcher = SkimMatcherV2::default().smart_case();
    let mut scored: Vec<(String, i64)> = Vec::new();
    for path in &index.files {
        let mut total: i64 = 0;
        let mut all_matched = true;
        for tok in &tokens {
            match matcher.fuzzy_match(path, tok) {
                Some(s) => total += s,
                None => {
                    all_matched = false;
                    break;
                }
            }
        }
        if all_matched {
            // Tiny bonus for shorter paths so file basenames win
            // when two candidates score equally on the user's query.
            let length_penalty = (path.len() as i64).min(200);
            scored.push((path.clone(), total * 100 - length_penalty));
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(RESULT_LIMIT);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        let dir = TempDir::new().unwrap();
        for path in [
            "src/main.rs",
            "src/lib/foo.rs",
            "lib/vfs/access_layer/smb/smb_share_vfs_posix.c",
            "lib/vfs/access_layer/smb/smb_share_vfs_posix.h",
            "lib/vfs/access_layer/cifs/cifs.c",
            "docs/readme.md",
            "tests/integration.rs",
        ] {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, "").unwrap();
        }
        dir
    }

    #[test]
    fn empty_query_returns_files_alphabetically() {
        let dir = fixture();
        let idx = Index::build(dir.path());
        let results = search(&idx, "");
        assert!(!results.is_empty());
        // First file (shortest path) is somewhere sensible.
        assert!(results.iter().any(|(p, _)| p == "docs/readme.md"));
    }

    #[test]
    fn multi_token_query_finds_smb_path() {
        let dir = fixture();
        let idx = Index::build(dir.path());
        let results = search(&idx, "posix.c share");
        let top = &results[0].0;
        // Both `posix.c` AND `share` substrings need to land in the path.
        // Two `.c` files contain `share` — but only one contains both.
        assert_eq!(top, "lib/vfs/access_layer/smb/smb_share_vfs_posix.c");
    }

    #[test]
    fn header_file_filtered_out_by_extension_token() {
        let dir = fixture();
        let idx = Index::build(dir.path());
        let results = search(&idx, "posix.c");
        // The `.h` file does not contain `.c` as a substring sequence,
        // so it must not be in the matches.
        assert!(results.iter().all(|(p, _)| !p.ends_with("posix.h")));
    }

    #[test]
    fn unmatched_token_kills_the_path() {
        let dir = fixture();
        let idx = Index::build(dir.path());
        let results = search(&idx, "posix.c nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn gitignore_is_respected() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap(); // need a git dir for `ignore` to engage
        fs::create_dir(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join("ignored/secret"), "").unwrap();
        fs::write(dir.path().join("visible.txt"), "").unwrap();
        let idx = Index::build(dir.path());
        assert!(idx.files.iter().any(|p| p == "visible.txt"));
        assert!(idx.files.iter().all(|p| !p.starts_with("ignored/")));
    }
}
