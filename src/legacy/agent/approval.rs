//! Approval gate — pauses tool execution until the user approves or denies.

use tokio::sync::oneshot;

/// Holds a pending approval request.
///
/// The agent loop awaits the receiver; the UI sends the decision via the sender.
#[derive(Debug)]
pub struct ApprovalGate {
    /// Tool name awaiting approval.
    pub tool_name: String,
    /// Serialised tool arguments.
    pub args: String,
    /// Channel to send the user's decision (true = approved, false = denied).
    sender: Option<oneshot::Sender<bool>>,
}

impl ApprovalGate {
    /// Create a new gate and return both the gate and the approval receiver.
    pub fn new(
        tool_name: impl Into<String>,
        args: impl Into<String>,
    ) -> (Self, oneshot::Receiver<bool>) {
        let (tx, rx) = oneshot::channel();
        let gate = Self {
            tool_name: tool_name.into(),
            args: args.into(),
            sender: Some(tx),
        };
        (gate, rx)
    }

    /// Resolve the gate with the user's decision.
    pub fn resolve(&mut self, approved: bool) {
        if let Some(tx) = self.sender.take() {
            let _ = tx.send(approved);
        }
    }
}
