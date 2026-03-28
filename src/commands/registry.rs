//! Slash command registry for Potato.
//!
//! Defines all built-in slash commands, parses user input and returns a
//! [`CommandResult`] that the input handler dispatches on.

// ── CommandCategory ───────────────────────────────────────────────────────────

/// Logical grouping for slash commands (used in help overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    /// Session lifecycle: /new, /sessions, /export
    Session,
    /// Navigation: /help
    Navigation,
    /// Agent context: /agent, /role
    Agent,
}

// ── OverlayKind ───────────────────────────────────────────────────────────────

/// Identifies which overlay to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayKind {
    Help,
    Sessions,
    /// Agent picker — list detected agents and launch one in a new pane.
    AgentPicker,
}

// ── CommandResult ─────────────────────────────────────────────────────────────

/// Outcome of parsing a slash command input string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// Command was handled internally; don't send to PTY.
    Handled,
    /// Show an overlay (help, sessions picker, etc.).
    ShowOverlay(OverlayKind),
    /// Spawn a new session, optionally for a named agent.
    NewSession { agent: Option<String> },
    /// Set a role on the current pane.
    SetRole { name: String, description: Option<String> },
    /// Input started with `/` but the command was not recognised.
    Unknown(String),
    /// Input does not start with `/`; pass through to the PTY as-is.
    PassThrough(String),
}

// ── SlashCommand ──────────────────────────────────────────────────────────────

/// Metadata for a single built-in slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    /// Canonical command name (without leading `/`), e.g. `"new"`.
    pub name: &'static str,
    /// Short aliases (without leading `/`), e.g. `&["n"]`.
    pub aliases: &'static [&'static str],
    /// One-line description shown in the autocomplete popup and help overlay.
    pub description: &'static str,
    /// Usage string shown in help, e.g. `"/new"` or `"/role <name> [description]"`.
    pub usage: &'static str,
    /// Logical category for grouping in the help overlay.
    pub category: CommandCategory,
}

impl SlashCommand {
    /// Returns `true` if `name_or_alias` (without leading `/`) matches this
    /// command's canonical name or any of its aliases.
    pub fn matches(&self, name_or_alias: &str) -> bool {
        if self.name.eq_ignore_ascii_case(name_or_alias) {
            return true;
        }
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name_or_alias))
    }
}

// ── Built-in command registry ─────────────────────────────────────────────────

static COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "new",
        aliases: &["n"],
        description: "Start a new Claude session",
        usage: "/new",
        category: CommandCategory::Session,
    },
    SlashCommand {
        name: "sessions",
        aliases: &["s"],
        description: "Show session picker",
        usage: "/sessions",
        category: CommandCategory::Session,
    },
    SlashCommand {
        name: "export",
        aliases: &["e"],
        description: "Export current session",
        usage: "/export",
        category: CommandCategory::Session,
    },
    SlashCommand {
        name: "help",
        aliases: &["?", "h"],
        description: "Show keyboard shortcuts and commands",
        usage: "/help",
        category: CommandCategory::Navigation,
    },
    SlashCommand {
        name: "agent",
        aliases: &["a"],
        description: "Show agent info",
        usage: "/agent",
        category: CommandCategory::Agent,
    },
    SlashCommand {
        name: "role",
        aliases: &["r"],
        description: "Set the role for the current pane",
        usage: "/role <name> [description]",
        category: CommandCategory::Agent,
    },
];

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns all built-in slash commands.
pub fn all_commands() -> &'static [SlashCommand] {
    COMMANDS
}

/// Parse a user input string into a [`CommandResult`].
///
/// - If `input` does not start with `/`, returns `PassThrough`.
/// - If `input` starts with `/` but matches no known command, returns `Unknown`.
/// - Otherwise dispatches to the appropriate handler.
pub fn parse_command(input: &str) -> CommandResult {
    let input = input.trim();

    if !input.starts_with('/') {
        return CommandResult::PassThrough(input.to_string());
    }

    // Strip the leading `/` and split into tokens.
    let rest = &input[1..];
    let mut tokens = rest.splitn(3, ' ');
    let cmd_name = tokens.next().unwrap_or("").trim();
    let arg1 = tokens.next().map(str::trim).filter(|s| !s.is_empty());
    let rest_args = tokens.next().map(str::trim).filter(|s| !s.is_empty());

    // Find the matching command.
    let cmd = COMMANDS.iter().find(|c| c.matches(cmd_name));

    match cmd {
        None => CommandResult::Unknown(cmd_name.to_string()),

        Some(c) => match c.name {
            "new" => CommandResult::NewSession { agent: arg1.map(str::to_string) },

            "sessions" => CommandResult::ShowOverlay(OverlayKind::Sessions),

            "export" => CommandResult::Handled,

            "help" => CommandResult::ShowOverlay(OverlayKind::Help),

            "agent" => CommandResult::ShowOverlay(OverlayKind::AgentPicker),

            "role" => {
                if let Some(name) = arg1 {
                    // description is everything after the name
                    let description = rest_args.map(str::to_string);
                    CommandResult::SetRole {
                        name: name.to_string(),
                        description,
                    }
                } else {
                    // /role with no arguments → show usage hint
                    CommandResult::Unknown("role <name> [description]".to_string())
                }
            }

            _ => CommandResult::Unknown(cmd_name.to_string()),
        },
    }
}

/// Return all commands whose name or alias starts with `prefix` (case-insensitive).
///
/// Used for autocomplete: the caller supplies the text typed after `/`.
/// Returns at most all commands, ordered: exact-name matches first, then alias
/// matches, then prefix matches.
pub fn completions(prefix: &str) -> Vec<&'static SlashCommand> {
    let prefix_lower = prefix.to_lowercase();

    if prefix_lower.is_empty() {
        return COMMANDS.iter().collect();
    }

    // Separate exact-name matches, alias matches, and prefix matches.
    let mut exact: Vec<&'static SlashCommand> = Vec::new();
    let mut alias_match: Vec<&'static SlashCommand> = Vec::new();
    let mut prefix_match: Vec<&'static SlashCommand> = Vec::new();

    for cmd in COMMANDS {
        if cmd.name == prefix_lower {
            exact.push(cmd);
        } else if cmd.aliases.iter().any(|a| *a == prefix_lower) {
            alias_match.push(cmd);
        } else if cmd.name.starts_with(&prefix_lower as &str)
            || cmd.aliases.iter().any(|a| a.starts_with(&prefix_lower as &str))
        {
            prefix_match.push(cmd);
        }
    }

    let mut result = exact;
    result.extend(alias_match);
    result.extend(prefix_match);
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── all_commands ──────────────────────────────────────────────────────────

    #[test]
    fn all_commands_is_nonempty() {
        assert!(!all_commands().is_empty());
    }

    #[test]
    fn all_commands_names_are_unique() {
        let names: Vec<_> = all_commands().iter().map(|c| c.name).collect();
        let mut dedup = names.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len(), "duplicate command names detected");
    }

    // ── parse_command: PassThrough ────────────────────────────────────────────

    #[test]
    fn parse_non_slash_input_is_passthrough() {
        assert_eq!(
            parse_command("hello world"),
            CommandResult::PassThrough("hello world".to_string())
        );
    }

    #[test]
    fn parse_empty_string_is_passthrough() {
        assert_eq!(
            parse_command(""),
            CommandResult::PassThrough("".to_string())
        );
    }

    // ── parse_command: /new ───────────────────────────────────────────────────

    #[test]
    fn parse_new_no_agent() {
        assert_eq!(
            parse_command("/new"),
            CommandResult::NewSession { agent: None }
        );
    }

    #[test]
    fn parse_new_alias_n() {
        assert_eq!(
            parse_command("/n"),
            CommandResult::NewSession { agent: None }
        );
    }

    #[test]
    fn parse_new_with_agent() {
        assert_eq!(
            parse_command("/new codex"),
            CommandResult::NewSession { agent: Some("codex".to_string()) }
        );
    }

    // ── parse_command: /sessions ──────────────────────────────────────────────

    #[test]
    fn parse_sessions() {
        assert_eq!(
            parse_command("/sessions"),
            CommandResult::ShowOverlay(OverlayKind::Sessions)
        );
    }

    #[test]
    fn parse_sessions_alias_s() {
        assert_eq!(
            parse_command("/s"),
            CommandResult::ShowOverlay(OverlayKind::Sessions)
        );
    }

    // ── parse_command: /export ────────────────────────────────────────────────

    #[test]
    fn parse_export() {
        assert_eq!(parse_command("/export"), CommandResult::Handled);
    }

    #[test]
    fn parse_export_alias_e() {
        assert_eq!(parse_command("/e"), CommandResult::Handled);
    }

    // ── parse_command: /help ──────────────────────────────────────────────────

    #[test]
    fn parse_help() {
        assert_eq!(
            parse_command("/help"),
            CommandResult::ShowOverlay(OverlayKind::Help)
        );
    }

    #[test]
    fn parse_help_alias_h() {
        assert_eq!(
            parse_command("/h"),
            CommandResult::ShowOverlay(OverlayKind::Help)
        );
    }

    #[test]
    fn parse_help_alias_question_mark() {
        assert_eq!(
            parse_command("/?"),
            CommandResult::ShowOverlay(OverlayKind::Help)
        );
    }

    // ── parse_command: /agent ─────────────────────────────────────────────────

    #[test]
    fn parse_agent() {
        assert_eq!(
            parse_command("/agent"),
            CommandResult::ShowOverlay(OverlayKind::AgentPicker)
        );
    }

    #[test]
    fn parse_agent_alias_a() {
        assert_eq!(
            parse_command("/a"),
            CommandResult::ShowOverlay(OverlayKind::AgentPicker)
        );
    }

    // ── parse_command: /role ──────────────────────────────────────────────────

    #[test]
    fn parse_role_name_only() {
        assert_eq!(
            parse_command("/role architect"),
            CommandResult::SetRole {
                name: "architect".to_string(),
                description: None,
            }
        );
    }

    #[test]
    fn parse_role_with_description() {
        assert_eq!(
            parse_command("/role architect Frontend API design"),
            CommandResult::SetRole {
                name: "architect".to_string(),
                description: Some("Frontend API design".to_string()),
            }
        );
    }

    #[test]
    fn parse_role_alias_r() {
        assert_eq!(
            parse_command("/r reviewer"),
            CommandResult::SetRole {
                name: "reviewer".to_string(),
                description: None,
            }
        );
    }

    #[test]
    fn parse_role_no_args_returns_unknown() {
        match parse_command("/role") {
            CommandResult::Unknown(_) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    // ── parse_command: Unknown ────────────────────────────────────────────────

    #[test]
    fn parse_unknown_command() {
        assert_eq!(
            parse_command("/foobar"),
            CommandResult::Unknown("foobar".to_string())
        );
    }

    // ── completions ───────────────────────────────────────────────────────────

    #[test]
    fn completions_empty_prefix_returns_all() {
        assert_eq!(completions("").len(), all_commands().len());
    }

    #[test]
    fn completions_h_matches_help() {
        let results = completions("h");
        assert!(
            results.iter().any(|c| c.name == "help"),
            "expected /help in completions for 'h', got: {:?}",
            results.iter().map(|c| c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_ro_matches_role() {
        let results = completions("ro");
        assert!(results.iter().any(|c| c.name == "role"), "expected /role");
    }

    #[test]
    fn completions_garbage_returns_empty() {
        let results = completions("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn completions_n_matches_new() {
        let results = completions("n");
        // 'n' is an alias for /new, and also prefix-matches 'new'
        assert!(results.iter().any(|c| c.name == "new"));
    }

    #[test]
    fn completions_exact_name_comes_first() {
        // "new" exact → should be the first result
        let results = completions("new");
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "new");
    }

    // ── SlashCommand::matches ─────────────────────────────────────────────────

    #[test]
    fn slash_command_matches_by_name() {
        let cmd = &COMMANDS[0]; // "new"
        assert!(cmd.matches("new"));
        assert!(!cmd.matches("unknown"));
    }

    #[test]
    fn slash_command_matches_by_alias() {
        let role_cmd = COMMANDS.iter().find(|c| c.name == "role").unwrap();
        assert!(role_cmd.matches("r"));
        assert!(role_cmd.matches("role"));
        assert!(!role_cmd.matches("ro"));
    }

    #[test]
    fn slash_command_matches_case_insensitive() {
        let cmd = COMMANDS.iter().find(|c| c.name == "help").unwrap();
        assert!(cmd.matches("HELP"));
        assert!(cmd.matches("Help"));
        assert!(cmd.matches("H")); // alias "?"... wait H not H alias
        assert!(cmd.matches("h")); // alias "h"
    }

    // ── parse_command: whitespace trimming ───────────────────────────────────

    #[test]
    fn parse_trims_leading_trailing_whitespace() {
        assert_eq!(
            parse_command("  /help  "),
            CommandResult::ShowOverlay(OverlayKind::Help)
        );
    }

    // ── parse_command: role with multi-word description ───────────────────────

    #[test]
    fn parse_role_multi_word_description() {
        match parse_command("/role devops Deploy pipeline & infra management") {
            CommandResult::SetRole { name, description } => {
                assert_eq!(name, "devops");
                assert_eq!(description, Some("Deploy pipeline & infra management".to_string()));
            }
            other => panic!("expected SetRole, got {:?}", other),
        }
    }
}
