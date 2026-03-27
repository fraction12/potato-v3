//! Actions are the output of the update function — side effects to be executed.

use crate::ui::panels::PanelId;

/// All possible actions that the update function can emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No operation — do nothing.
    Noop,
    /// Quit the application cleanly.
    Quit,
    /// Send the current input buffer as a message to the agent.
    SendMessage(String),
    /// Focus a specific panel by index (legacy — prefer FocusPanelById).
    FocusPanel(usize),
    /// Move focus to the next panel in the focus ring.
    FocusNextPanel,
    /// Move focus to the previous panel in the focus ring.
    FocusPreviousPanel,
    /// Toggle visibility of a panel by id.
    TogglePanel(PanelId),
    /// Open the slash command overlay.
    OpenSlashMenu,
    /// Open the model picker overlay.
    OpenModelPicker,
    /// Open the help overlay.
    OpenHelp,
    /// Approve a pending tool call.
    ApproveToolCall,
    /// Approve all future tool calls of this type without asking.
    ApproveAllToolCalls,
    /// Deny a pending tool call.
    DenyToolCall,
    /// Scroll the active panel up by one line.
    ScrollUp,
    /// Scroll the active panel down by one line.
    ScrollDown,
    /// Scroll to the very top of the chat.
    ScrollTop,
    /// Scroll to the very bottom of the chat (auto-follow).
    ScrollBottom,
    /// Clear the input buffer.
    ClearInput,
    /// Switch to the previous session.
    PreviousSession,
    /// Switch to the next session.
    NextSession,
    /// Create a new session.
    NewSession,
    /// Move the input cursor left by one character.
    InputCursorLeft,
    /// Move the input cursor right by one character.
    InputCursorRight,
    /// Move the input cursor to the start of the line.
    InputCursorHome,
    /// Move the input cursor to the end of the line.
    InputCursorEnd,
    /// Delete the character before the cursor.
    InputBackspace,
    /// Insert a character at the cursor position.
    InputInsert(char),
    /// Toggle expansion of a tool card in the chat.
    ToggleToolCard(usize),
}
