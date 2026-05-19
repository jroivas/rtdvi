//! Search state shared by `/` (forward) and `?` (backward).
//!
//! `pattern` is the last successfully-compiled regex; `prompt` is the
//! in-progress input while the search prompt is open. `direction` tracks
//! whether the most recent search was forward or backward so `n` repeats
//! correctly.

use regex::Regex;

#[derive(Default)]
pub struct SearchState {
    pub prompt: String,
    pub prompt_cursor: usize,
    /// Compiled from `prompt` on every keystroke for incremental highlighting.
    pub prompt_re: Option<Regex>,
    pub pattern: Option<Regex>,
    pub last_pattern: Option<String>,
    pub direction_forward: bool,
}

impl SearchState {
    pub fn clear_prompt(&mut self) {
        self.prompt.clear();
        self.prompt_cursor = 0;
        self.prompt_re = None;
    }

    /// Recompile `prompt_re` from the current `prompt`. Invalid patterns leave
    /// `prompt_re` as `None` so rendering silently shows no incremental matches.
    pub fn update_prompt_re(&mut self) {
        self.prompt_re = if self.prompt.is_empty() {
            None
        } else {
            Regex::new(&self.prompt).ok()
        };
    }

    pub fn set_pattern(&mut self, pat: &str) -> Result<(), regex::Error> {
        let re = Regex::new(pat)?;
        self.pattern = Some(re);
        self.last_pattern = Some(pat.to_string());
        Ok(())
    }
}

impl std::fmt::Debug for SearchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchState")
            .field("prompt", &self.prompt)
            .field("prompt_cursor", &self.prompt_cursor)
            .field("last_pattern", &self.last_pattern)
            .field("direction_forward", &self.direction_forward)
            .finish()
    }
}

impl Clone for SearchState {
    fn clone(&self) -> Self {
        Self {
            prompt: self.prompt.clone(),
            prompt_cursor: self.prompt_cursor,
            prompt_re: self.prompt.is_empty().then_some(()).map_or_else(
                || Regex::new(&self.prompt).ok(),
                |_| None,
            ),
            pattern: self
                .last_pattern
                .as_deref()
                .and_then(|p| Regex::new(p).ok()),
            last_pattern: self.last_pattern.clone(),
            direction_forward: self.direction_forward,
        }
    }
}
