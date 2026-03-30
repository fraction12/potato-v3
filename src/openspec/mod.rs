//! OpenSpec integration — reads `openspec/changes/*/tasks.md` and provides
//! live task data to the UI and MCP layer.

mod parser;
mod watcher;

pub use parser::{OpenSpecBacklog, OpenSpecTask, TaskStatus};
pub use watcher::OpenSpecWatcher;
