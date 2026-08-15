//! A tab page: a split tree plus the currently focused window.

use crate::window::{SplitTree, WindowId};

#[derive(Debug)]
pub struct Tab {
    pub tree: SplitTree,
    pub active: WindowId,
    /// True once the user has manually changed a split ratio in this tab
    /// (`:resize`, `<C-w>_`, `<C-w>|`). While set, opening/closing splits no
    /// longer auto-equalizes the layout, so the custom sizing is preserved.
    /// Cleared by `<C-w>=` (an explicit re-equalize) and when the tab collapses
    /// back to a single window.
    pub manually_resized: bool,
}

impl Tab {
    pub fn single(window: WindowId) -> Self {
        Self {
            tree: SplitTree::leaf(window),
            active: window,
            manually_resized: false,
        }
    }
}
