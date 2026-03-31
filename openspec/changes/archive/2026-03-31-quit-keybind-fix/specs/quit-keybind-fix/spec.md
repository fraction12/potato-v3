## ADDED Requirements

### Requirement: Quit keybind documentation matches handler

All user-facing quit keybind references (help overlay, footer bars, dashboard hints, doc comments) and the default keybind config SHALL display `Ctrl+Q` as the quit binding. No reference to `Ctrl+\` SHALL remain in UI text or default configuration.

#### Scenario: Help overlay shows Ctrl+Q
- **WHEN** the user opens the help overlay
- **THEN** global and terminal-focus quit entries SHALL display `Ctrl+Q`

#### Scenario: Dashboard footer shows Ctrl+Q
- **WHEN** the user is on the dashboard screen
- **THEN** the footer hint bar SHALL show `Ctrl+Q` as the quit binding

#### Scenario: Session footer shows Ctrl+Q
- **WHEN** the user is in a session (single or multi-pane)
- **THEN** the session status bar SHALL show `Ctrl+Q` as the quit binding

#### Scenario: Default keybind config is Ctrl+Q
- **WHEN** no user config overrides the quit binding
- **THEN** the default quit keybind SHALL be `ctrl+q`
