//! A tab page: a split tree plus the currently focused window.

use crate::window::{SplitTree, WindowId};

#[derive(Debug)]
pub struct Tab {
    pub tree: SplitTree,
    pub active: WindowId,
}

impl Tab {
    pub fn single(window: WindowId) -> Self {
        Self {
            tree: SplitTree::leaf(window),
            active: window,
        }
    }
}
