//! Search state shared by `/` and `?`. Stub — implemented in M8.

#[derive(Default, Debug, Clone)]
pub struct SearchState {
    pub last_pattern: Option<String>,
    pub backward: bool,
}
