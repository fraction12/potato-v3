//! Slash command system for the Potato input bar.
//!
//! When the user types `/` as the first character in the input bar, the command
//! engine activates:
//! - [`registry::parse_command`] parses input into a [`CommandResult`]
//! - [`registry::completions`] returns fuzzy-matched suggestions for autocomplete
//! - [`registry::all_commands`] provides the full command list for the help overlay

pub mod registry;

pub use registry::{CommandCategory, CommandResult, OverlayKind, SlashCommand};
