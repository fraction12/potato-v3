# Tasks — phase-9-release

- [x] T-901: Error handling and resilience — agent crash recovery, PTY read errors, config validation, graceful degradation
- [ ] T-902: Responsive layouts and compact mode — compact mode below 80x24, breakpoints for panel collapse, resize stress testing (PARTIAL: presets exist, no min-size guard)
- [ ] T-903: Distribution — cargo install, Homebrew tap, GitHub Actions CI/CD, cross-compilation for macOS ARM/Intel + Linux
- [ ] T-904: README + screenshots + GIFs — hero GIF via vhs/asciinema, feature screenshots, install instructions, quickstart guide
- [ ] T-905: Switch OpenSpec watcher to poll backend — replace kqueue (panics on rapid creates/deletes) with PollWatcher for backlog file (PARTIAL: snapshot via CLI, no file watcher)
- [x] T-906: Redirect stderr to log file at TUI startup — dup2 stderr to ~/.potato/potato.log to prevent background output corrupting ratatui surface
- [x] T-907: Background thread panic force full TUI redraw — enhance panic hook to signal main loop for terminal.clear() + full redraw instead of exit
