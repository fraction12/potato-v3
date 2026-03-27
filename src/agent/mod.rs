//! Agent loop — drives the AI reasoning and tool-use cycle.

pub mod approval;
pub mod loop_;
pub mod state_machine;
pub mod streaming;

pub use loop_::agent_loop;
pub use state_machine::AgentState;
pub use streaming::StreamAccumulator;
