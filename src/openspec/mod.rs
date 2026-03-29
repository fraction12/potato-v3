//! OpenSpec integration — reads `.openspec/backlog.yaml` and syncs
//! tickets with the MCP task board.

mod parser;
mod watcher;

pub use parser::{OpenSpecBacklog, OpenSpecTask, TaskStatus};
pub use watcher::OpenSpecWatcher;
