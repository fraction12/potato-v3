# Tasks — phase-10-keybind-overhaul

- [ ] T-1001: Remove bare-letter shortcuts and fix ? binding — remove q/j/k/r/? bare keys that block typing; rework a/d and Esc behavior
- [ ] T-1002: Add F-key shortcuts — F1 Help, F5 Refresh, F6 Focus Terminal replacing removed bare-letter bindings
- [ ] T-1003: Extract input_handler.rs — centralize all keybind dispatch out of main.rs into testable module with InputAction enum
- [ ] T-1004: Quick Actions panel in sidebar — context-sensitive action list with keybind hints, Arrow/Enter to execute
