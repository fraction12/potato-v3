//! Modal overlays that float above the panel layout.

pub mod agent_picker;
pub mod confirm;
pub mod help;
pub mod model_picker;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};

// ── OverlayAction ─────────────────────────────────────────────────────────────

/// Actions that an overlay can emit after processing a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAction {
    /// Do nothing.
    None,
    /// Close this overlay and remove it from the stack.
    Close,
    /// User selected an item (carries the selected string).
    Select(String),
    /// User confirmed (true) or cancelled (false) a yes/no prompt.
    Confirm(bool),
}

// ── Overlay trait ─────────────────────────────────────────────────────────────

/// Trait implemented by all modal overlays.
pub trait Overlay: Send {
    /// Render the overlay centred over `area`.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Handle a key event while this overlay is active.
    ///
    /// The default implementation closes on Esc; overlay implementations
    /// should call this or handle Esc themselves.
    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction;

    /// Short title shown in the overlay border.
    fn title(&self) -> &str;
}

// ── OverlayStack ──────────────────────────────────────────────────────────────

/// A stack of modal overlays.
///
/// The top overlay captures all key events.  Pushing a new overlay places it
/// on top; popping (or closing via Esc) removes the top one.
pub struct OverlayStack {
    stack: Vec<Box<dyn Overlay>>,
}

impl Default for OverlayStack {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayStack {
    /// Create a new, empty overlay stack.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a new overlay onto the stack.
    pub fn push(&mut self, overlay: Box<dyn Overlay>) {
        self.stack.push(overlay);
    }

    /// Pop the top overlay, returning it (or `None` if the stack is empty).
    pub fn pop(&mut self) -> Option<Box<dyn Overlay>> {
        self.stack.pop()
    }

    /// Returns `true` if there are any open overlays.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Number of overlays currently on the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Peek at the top overlay (immutable).
    #[must_use]
    pub fn top(&self) -> Option<&dyn Overlay> {
        self.stack.last().map(|b| b.as_ref())
    }

    /// Peek at the top overlay (mutable).
    pub fn top_mut(&mut self) -> Option<&mut dyn Overlay> {
        match self.stack.last_mut() {
            Some(b) => Some(b.as_mut()),
            None => None,
        }
    }

    /// Dispatch a key event to the top overlay.
    ///
    /// Returns the [`OverlayAction`] produced by the overlay.
    /// If the action is [`OverlayAction::Close`], the top overlay is
    /// automatically popped from the stack.
    /// Esc always closes the top overlay.
    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        // Esc always closes top overlay.
        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            if !self.stack.is_empty() {
                self.stack.pop();
                return OverlayAction::Close;
            }
            return OverlayAction::None;
        }

        if let Some(top) = self.stack.last_mut() {
            let action = top.handle_key(key);
            if action == OverlayAction::Close {
                self.stack.pop();
            }
            action
        } else {
            OverlayAction::None
        }
    }

    /// Render all overlays from bottom to top, each centred over `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        for overlay in &self.stack {
            overlay.render(frame, area);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Minimal stub overlay for testing ─────────────────────────────────────

    struct StubOverlay {
        title: &'static str,
        /// If true, the overlay returns Close on any key press.
        closes_on_any_key: bool,
    }

    impl StubOverlay {
        fn new(title: &'static str) -> Self {
            Self {
                title,
                closes_on_any_key: false,
            }
        }

        fn closing(title: &'static str) -> Self {
            Self {
                title,
                closes_on_any_key: true,
            }
        }
    }

    impl Overlay for StubOverlay {
        fn render(&self, _frame: &mut Frame, _area: Rect) {}

        fn handle_key(&mut self, _key: KeyEvent) -> OverlayAction {
            if self.closes_on_any_key {
                OverlayAction::Close
            } else {
                OverlayAction::None
            }
        }

        fn title(&self) -> &str {
            self.title
        }
    }

    fn esc_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    // ── test_overlay_stack_push_pop ───────────────────────────────────────────

    #[test]
    fn test_overlay_stack_push_pop() {
        let mut stack = OverlayStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);

        stack.push(Box::new(StubOverlay::new("first")));
        assert!(!stack.is_empty());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.top().unwrap().title(), "first");

        stack.push(Box::new(StubOverlay::new("second")));
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.top().unwrap().title(), "second");

        let popped = stack.pop();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().title(), "second");
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.top().unwrap().title(), "first");

        stack.pop();
        assert!(stack.is_empty());
    }

    // ── test_overlay_esc_closes_top ───────────────────────────────────────────

    #[test]
    fn test_overlay_esc_closes_top() {
        let mut stack = OverlayStack::new();
        stack.push(Box::new(StubOverlay::new("bottom")));
        stack.push(Box::new(StubOverlay::new("top")));
        assert_eq!(stack.len(), 2);

        let action = stack.handle_key(esc_key());
        assert_eq!(action, OverlayAction::Close);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.top().unwrap().title(), "bottom");

        let action = stack.handle_key(esc_key());
        assert_eq!(action, OverlayAction::Close);
        assert!(stack.is_empty());
    }

    #[test]
    fn test_overlay_esc_on_empty_stack_is_noop() {
        let mut stack = OverlayStack::new();
        let action = stack.handle_key(esc_key());
        assert_eq!(action, OverlayAction::None);
    }

    #[test]
    fn test_overlay_close_action_auto_pops() {
        let mut stack = OverlayStack::new();
        stack.push(Box::new(StubOverlay::closing("auto-close")));
        assert_eq!(stack.len(), 1);

        // Pressing any key causes the overlay to return Close, which should
        // auto-pop it.
        let action = stack.handle_key(char_key('x'));
        assert_eq!(action, OverlayAction::Close);
        assert!(stack.is_empty());
    }
}
