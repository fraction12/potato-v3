//! Legacy agent loop — retired from main code path; preserved for test coverage.

pub mod approval;
pub mod loop_;
pub mod state_machine;
pub mod streaming;

pub use loop_::agent_loop;
pub use state_machine::AgentState;
pub use streaming::StreamAccumulator;
