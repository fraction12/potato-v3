//! Actions are the output of the update function — side effects to be executed.

/// All possible actions that the update function can emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No operation — do nothing.
    Noop,
    /// Quit the application cleanly.
    Quit,
    /// Send the current input buffer as a message to the agent.
    SendMessage(String),
    /// Focus a specific panel by index.
    FocusPanel(usize),
    /// Open the slash command overlay.
    OpenSlashMenu,
    /// Open the model picker overlay.
    OpenModelPicker,
    /// Open the help overlay.
    OpenHelp,
    /// Approve a pending tool call.
    ApproveToolCall,
    /// Deny a pending tool call.
    DenyToolCall,
    /// Scroll the active panel up.
    ScrollUp,
    /// Scroll the active panel down.
    ScrollDown,
    /// Clear the input buffer.
    ClearInput,
    /// Switch to the previous session.
    PreviousSession,
    /// Switch to the next session.
    NextSession,
    /// Create a new session.
    NewSession,
}
