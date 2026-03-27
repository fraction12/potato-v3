//! Built-in tools shipped with Potato.

pub mod edit_file;
pub mod list_dir;
pub mod read_file;
pub mod search;
pub mod shell;
pub mod write_file;

use std::sync::Arc;

use crate::tools::registry::ToolRegistry;

use self::{
    edit_file::EditFileTool,
    list_dir::ListDirTool,
    read_file::ReadFileTool,
    search::SearchTool,
    shell::ShellTool,
    write_file::WriteFileTool,
};

/// Register all built-in tools into the given [`ToolRegistry`].
///
/// Call this once during application startup after creating the registry.
pub fn register_builtins(registry: &mut ToolRegistry) {
    registry.register(Arc::new(ShellTool));
    registry.register(Arc::new(ReadFileTool));
    registry.register(Arc::new(WriteFileTool));
    registry.register(Arc::new(EditFileTool));
    registry.register(Arc::new(SearchTool));
    registry.register(Arc::new(ListDirTool));
}
