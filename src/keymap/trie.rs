//! Trie keyed on key sequences.

use std::collections::HashMap;

use super::{Action, Key};

#[derive(Default, Debug)]
pub struct KeyTrie {
    children: HashMap<Key, KeyTrieNode>,
}

#[derive(Debug)]
pub enum KeyTrieNode {
    Leaf(Action),
    Branch(KeyTrie),
    /// Some key maps to an action *and* can be a prefix of another binding.
    /// In that case `action` resolves on this key alone; a longer pending
    /// sequence continues into `branch`. Vim has cases like `g` actions plus
    /// `gg`, `gE`, etc. — we resolve immediately on partial mismatch.
    Both { action: Action, branch: KeyTrie },
}

#[derive(Debug)]
pub enum Resolve {
    /// No binding for this prefix.
    None,
    /// Pending — more keys may complete a binding.
    Pending,
    /// Sequence matched; here's the action.
    Matched(Action),
}

impl KeyTrie {
    pub fn insert(&mut self, keys: &[Key], action: Action) {
        if keys.is_empty() {
            return;
        }
        let (first, rest) = (keys[0], &keys[1..]);
        let entry = self.children.remove(&first);
        let new_node = match (entry, rest.is_empty()) {
            (None, true) => KeyTrieNode::Leaf(action),
            (None, false) => {
                let mut t = KeyTrie::default();
                t.insert(rest, action);
                KeyTrieNode::Branch(t)
            }
            (Some(KeyTrieNode::Leaf(prev)), true) => KeyTrieNode::Leaf(action.replace(prev)),
            (Some(KeyTrieNode::Leaf(prev)), false) => {
                let mut t = KeyTrie::default();
                t.insert(rest, action);
                KeyTrieNode::Both { action: prev, branch: t }
            }
            (Some(KeyTrieNode::Branch(t)), true) => KeyTrieNode::Both { action, branch: t },
            (Some(KeyTrieNode::Branch(mut t)), false) => {
                t.insert(rest, action);
                KeyTrieNode::Branch(t)
            }
            (Some(KeyTrieNode::Both { action: prev, branch }), true) => KeyTrieNode::Both {
                action: action.replace(prev),
                branch,
            },
            (Some(KeyTrieNode::Both { action: prev, mut branch }), false) => {
                branch.insert(rest, action);
                KeyTrieNode::Both { action: prev, branch }
            }
        };
        self.children.insert(first, new_node);
    }

    pub fn resolve(&self, pending: &[Key]) -> Resolve {
        if pending.is_empty() {
            return Resolve::Pending;
        }
        let mut node = match self.children.get(&pending[0]) {
            Some(n) => n,
            None => return Resolve::None,
        };
        for key in &pending[1..] {
            let next_trie = match node {
                KeyTrieNode::Leaf(_) => return Resolve::None,
                KeyTrieNode::Branch(t) => t,
                KeyTrieNode::Both { branch, .. } => branch,
            };
            node = match next_trie.children.get(key) {
                Some(n) => n,
                None => return Resolve::None,
            };
        }
        match node {
            KeyTrieNode::Leaf(a) => Resolve::Matched(a.clone()),
            KeyTrieNode::Branch(_) => Resolve::Pending,
            // We could either fire the action now or wait. Wait — vim
            // resolves on the next non-matching key or timeout. For v1 we
            // resolve when there's no longer continuation that could match.
            KeyTrieNode::Both { .. } => Resolve::Pending,
        }
    }

    /// Resolve "as far as possible" — if a pending key cannot extend any
    /// binding, fall back to the `Both` action at the previous node.
    /// This is the "timeoutlen expired" / "ambiguity resolved" entry point.
    pub fn resolve_with_fallback(&self, pending: &[Key]) -> Resolve {
        match self.resolve(pending) {
            Resolve::Pending if !pending.is_empty() => {
                // If the node is a `Both`, prefer its action; otherwise stay pending.
                let mut node = self.children.get(&pending[0]).unwrap();
                for key in &pending[1..] {
                    let next = match node {
                        KeyTrieNode::Branch(t) | KeyTrieNode::Both { branch: t, .. } => {
                            t.children.get(key)
                        }
                        _ => None,
                    };
                    match next {
                        Some(n) => node = n,
                        None => return Resolve::None,
                    }
                }
                match node {
                    KeyTrieNode::Both { action, .. } => Resolve::Matched(action.clone()),
                    _ => Resolve::Pending,
                }
            }
            other => other,
        }
    }
}

impl Action {
    /// Replace one action with another, returning the new value. Used while
    /// rebuilding tree nodes during `insert`.
    fn replace(self, _old: Action) -> Action {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::*;
    use super::*;

    fn key(c: char) -> Key {
        Key::char(c)
    }

    #[test]
    fn resolve_single() {
        let mut t = KeyTrie::default();
        t.insert(&[key('h')], Action::Builtin("move_left"));
        match t.resolve(&[key('h')]) {
            Resolve::Matched(Action::Builtin("move_left")) => {}
            other => panic!("expected matched, got {other:?}"),
        }
    }

    #[test]
    fn resolve_pending() {
        let mut t = KeyTrie::default();
        t.insert(&[key('g'), key('g')], Action::Builtin("first_line"));
        match t.resolve(&[key('g')]) {
            Resolve::Pending => {}
            other => panic!("expected pending, got {other:?}"),
        }
        match t.resolve(&[key('g'), key('g')]) {
            Resolve::Matched(_) => {}
            other => panic!("expected matched, got {other:?}"),
        }
    }

    #[test]
    fn resolve_none() {
        let mut t = KeyTrie::default();
        t.insert(&[key('h')], Action::Builtin("move_left"));
        match t.resolve(&[key('Z')]) {
            Resolve::None => {}
            other => panic!("expected none, got {other:?}"),
        }
    }
}
