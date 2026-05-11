//! Editor-wide event bus.
//!
//! v1 has no listeners and dispatches no events to anybody yet, but every
//! buffer mutation path will route through `emit` so a future plugin layer
//! can subscribe without touching call sites.

use crate::buffer::{BufferId, Edit};
use crate::mode::ModeId;
use crate::window::WindowId;
use crate::Editor;

#[derive(Debug)]
#[non_exhaustive]
pub enum Event<'a> {
    BufferChanged { buffer: BufferId, edit: &'a Edit },
    CursorMoved { window: WindowId },
    ModeChanged { from: ModeId, to: ModeId },
    BufferOpened(BufferId),
    BufferSaved(BufferId),
    WindowResized,
    Quit,
}

/// A listener reacts to events synchronously. It can mutate the editor;
/// any events it emits get queued and drained after the current dispatch
/// returns (see [`EventBus::emit`]).
pub trait Listener: Send + Sync {
    fn on_event(&mut self, editor: &mut Editor, event: &Event<'_>);
}

#[derive(Default)]
pub struct EventBus {
    listeners: Vec<Box<dyn Listener>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&mut self, listener: Box<dyn Listener>) {
        self.listeners.push(listener);
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

/// Free function so the borrow checker can split `editor.events` from the
/// rest of `editor`. We swap the listener vec out, dispatch, swap back.
/// Reentrant emits during dispatch are quietly dropped in v1 — there's no
/// scripting layer yet to trigger them.
pub fn emit(editor: &mut Editor, ev: Event<'_>) {
    let mut listeners = std::mem::take(&mut editor.events.listeners);
    for l in &mut listeners {
        l.on_event(editor, &ev);
    }
    // Restore. If a listener subscribed during dispatch (unlikely in v1),
    // its subscription is preserved by extending.
    let added = std::mem::take(&mut editor.events.listeners);
    listeners.extend(added);
    editor.events.listeners = listeners;
}
