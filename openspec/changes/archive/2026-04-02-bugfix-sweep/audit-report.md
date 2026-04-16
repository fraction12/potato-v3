# Bugfix Sweep Audit Report

| Ticket | Severity | Status | One-line reason |
|---|---|---|---|
| T-850 | CRITICAL | FIXED | `read_new_bytes` advances offset by `consumed_len`, not full joined buffer length. |
| T-851 | CRITICAL | FIXED | `spawn_turn` now sends `AgentEvent::Error` when stdin prompt write fails. |
| T-852 | CRITICAL | FIXED | `ApprovalDecision` branches on `approved`; denied decisions no longer resume thinking. |
| T-853 | CRITICAL | FIXED | MCP inboxes are capped and `get_messages(..., true)` drains read entries from the front. |
| T-854 | CRITICAL | FIXED | Bridge parse errors are serialized with `serde_json`, eliminating JSON-string injection. |
| T-855 | CRITICAL | FIXED | Logging uses `SharedWriter(Arc<Mutex<File>>)` instead of `try_clone().unwrap()` per write. |
| T-856 | HIGH | FIXED | `MessageHistory::push` appends the row returned by `save_message`; it does not reload the whole table. |
| T-857 | HIGH | FIXED | `new_id()` uses UUID v4. |
| T-858 | HIGH | STILL OPEN | Log trackers do not check for file shrink/truncation before reading from the current offset. |
| T-859 | HIGH | FIXED | Markdown export escapes heading/code-fence-leading content before writing message bodies. |
| T-860 | HIGH | FIXED | `handle_send_message` resolves target and sends under one mutex lock. |
| T-861 | HIGH | FIXED | `send_message` rejects unregistered `to_pane` IDs before creating an inbox. |
| T-862 | HIGH | STILL OPEN | `inject_into_pane` still accepts a `usize` pane index while surrounding code also uses `u64` pane IDs. |
| T-863 | HIGH | STILL OPEN | `TerminalGuard::enter()` still calls `enable_raw_mode()` while ratatui init also handles terminal mode. |
| T-864 | HIGH | STILL OPEN | `_dirty_rx` is still intentionally unused/dropped, so dirty notifications are ineffective. |
| T-865 | HIGH | FIXED | Pending enter submissions are tracked by stable `pane_id` and re-resolved at send time. |
| T-866 | HIGH | FIXED | UI code uses character-safe truncation and char-index to byte-offset conversion. |
| T-867 | HIGH | FIXED | `build_agent_rows()` calls `detect()` once per agent and reuses the result. |
| T-868 | MEDIUM | STILL OPEN | PTY stderr reader task still has no kill-signal-driven shutdown path. |
| T-869 | MEDIUM | STILL OPEN | Keybind configuration is still raw strings without validation. |
| T-870 | MEDIUM | FIXED | MCP parse failures use `PARSE_ERROR` (`-32700`). |
| T-871 | MEDIUM | FIXED | Bridge only injects after `potato_send_message` when RPC response has no `error`. |
| T-872 | MEDIUM | FIXED | Bridge parses the inner RPC JSON once and reuses `parsed_rpc`. |
| T-873 | MEDIUM | FIXED | Partner resolution uses ordered `known_panes`, not `HashMap` iteration. |
| T-874 | MEDIUM | STILL OPEN | `TextDone` handling still targets the most recent assistant entry rather than a turn-specific record. |
| T-875 | MEDIUM | STILL OPEN | Session binding still replaces the in-memory `session_id` without remapping an earlier persisted row. |
| T-876 | MEDIUM | OBSOLETE | The `MAX_AGENTS = 2` pattern no longer exists in the current codebase. |
| T-877 | MEDIUM | FIXED | Input cursor tracks characters; rendering converts char index to byte offset safely. |
| T-878 | MEDIUM | FIXED | `in_code_block` is reset per transcript entry in chat rendering. |
| T-879 | MEDIUM | FIXED | Help/FilePreview now cache measured visible height from render instead of relying on a fixed value during normal use. |
| T-880 | LOW | STILL OPEN | Duplicate `truncate_str` helpers still exist in multiple modules. |
| T-881 | LOW | STILL OPEN | `compact_json` in `claude_log.rs` still truncates by byte slice (`s[..157]`). |
| T-882 | LOW | FIXED | `get_partner_status` includes roleless panes by iterating `known_panes` and synthesizing `unassigned`. |
| T-883 | LOW | FIXED | `initialized` returns an empty string response (notification semantics). |
| T-884 | LOW | STILL OPEN | Claude tool-result parsing still assumes string content; array content blocks are not handled. |
| T-885 | LOW | OBSOLETE | `pane_index_after_open` no longer exists anywhere in `main.rs` or the repo. |
| T-886 | LOW | STILL OPEN | Help overlay still repeats bindings such as `Tab`, `Shift+Tab`, and `Ctrl+Q` across sections. |
| T-887 | LOW | FIXED | `TokenSparkline` now stores history in `VecDeque` and pops from the front in O(1). |

## T-850 — CRITICAL
Ticket: Fix carry bytes double-counted in log offset — claude_log.rs/codex_log.rs offset advanced by full buf len including carry, causing bytes parsed twice
Status: **FIXED**

Evidence:
- `src/claude_log.rs`: the reader joins carry with fresh bytes, computes `consumed_len = joined.len() - self.carry.len()`, and advances `self.offset += consumed_len as u64`.
- `src/codex_log.rs`: same pattern.

That means the offset only advances by newly consumed bytes, excluding trailing carry retained for the next read.

## T-851 — CRITICAL
Ticket: Fix spawn_turn stdin write errors silently swallowed — caller hangs indefinitely; mirror spawn pattern and broadcast AgentEvent::Error
Status: **FIXED**

Evidence:
- `src/pty/mod.rs:366-372`
  ```rust
  tokio::spawn(async move {
      if let Err(e) = stdin.write_all(prompt_bytes.as_bytes()).await {
          error!(error = %e, "spawn_turn: failed to write prompt to stdin");
          let _ = event_tx_stdin.send(AgentEvent::Error {
              message: format!("stdin write failed: {e}"),
          });
      }
  });
  ```

The failure is no longer only logged; an `AgentEvent::Error` is emitted.

## T-852 — CRITICAL
Ticket: Fix ApprovalDecision ignores approved field — denied approvals treated same as approvals; branch on approved flag
Status: **FIXED**

Evidence:
- `src/app/session_reducer.rs:111-118` branches on `approved`:
  ```rust
  AgentEvent::ApprovalDecision { tool_id: _, approved } => {
      session.approval_pending = None;
      if approved {
  ```
- The denied branch sets the session back to idle instead of resuming the tool flow.

## T-853 — CRITICAL
Ticket: Fix unbounded MCP inbox growth — get_messages marks read but never removes; enforce cap or drain read messages
Status: **FIXED**

Evidence:
- `src/mcp/state.rs:222-225` caps inbox size:
  ```rust
  const MAX_INBOX: usize = 1000;
  while inbox.len() > MAX_INBOX {
      inbox.pop_front();
  }
  ```
- `src/mcp/state.rs` `get_messages(..., true)` now drains fully-read messages from the front:
  ```rust
  while queue.front().is_some_and(|m| m.read) {
      queue.pop_front();
  }
  ```

## T-854 — CRITICAL
Ticket: Fix JSON injection in bridge parse-error response — raw format!() with serde error message breaks JSON string; use proper serialization
Status: **FIXED**

Evidence:
- `src/mcp/bridge.rs:199-206`
  ```rust
  let err_resp = serde_json::json!({
      "jsonrpc": "2.0",
      "id": null,
      "error": { "code": -32700, "message": format!("Parse error: {e}") }
  });
  ```
- The outer `BridgeResponse` is also serialized with `serde_json::to_string`.

The error message is no longer interpolated into raw JSON text.

## T-855 — CRITICAL
Ticket: Fix file.try_clone().unwrap() in logging writer — allocates fd on every write, panics on fd exhaustion; use Mutex<File> with MakeWriter
Status: **FIXED**

Evidence:
- `src/log.rs:28-36`
  ```rust
  #[derive(Clone)]
  struct SharedWriter(Arc<Mutex<File>>);

  impl<'a> fmt::MakeWriter<'a> for SharedWriter {
      type Writer = SharedWriterGuard<'a>;
      fn make_writer(&'a self) -> Self::Writer {
          SharedWriterGuard(self.0.lock().expect("log mutex poisoned"))
      }
  }
  ```

There is no per-write `try_clone()` path anymore.

## T-856 — HIGH
Ticket: Fix MessageHistory::push reloads entire table — O(N) SELECT after INSERT; use last_insert_rowid() instead
Status: **FIXED**

Evidence:
- `src/session/history.rs:50-56`
  ```rust
  pub fn push(&mut self, role: &str, content: &str, tokens: Option<u32>) -> Result<()> {
      let msg = self
          .store
          .save_message(&self.session_id, role, content, tokens)?;
      if let Some(t) = msg.tokens {
          self.total_tokens += t;
      }
  ```
- The function appends the returned row; it does not call `load_messages()` or otherwise reload the table.

## T-857 — HIGH
Ticket: Fix new_id() generates non-unique IDs — nanosecond timestamp not collision-free on macOS; use UUID v4
Status: **FIXED**

Evidence:
- `src/session/store.rs:400-402`
  ```rust
  fn new_id() -> String {
      uuid::Uuid::new_v4().to_string()
  }
  ```

## T-858 — HIGH
Ticket: Fix no log rotation detection — tracker permanently stuck after file shrink/truncation; compare file length to offset, reset if shrunk
Status: **STILL OPEN**

Evidence:
- In both `src/claude_log.rs` and `src/codex_log.rs`, the trackers seek/read from `self.offset` and update it, but there is no pre-read comparison of current file length against the stored offset to detect shrink/truncation.
- No code path resets `self.offset` when the underlying file becomes shorter.

## T-859 — HIGH
Ticket: Fix raw message content in Markdown export without escaping — backticks/headings/HTML corrupt exported document
Status: **FIXED**

Evidence:
- `src/session/export.rs:72-79`
  ```rust
  fn escape_markdown_content(content: &str) -> String {
      content
          .lines()
          .map(|line| {
              if line.starts_with('#') || line.starts_with("```") {
                  format!("\\{line}")
  ```
- `export_markdown()` writes `escape_markdown_content(&msg.content)` instead of raw content.

## T-860 — HIGH
Ticket: Fix TOCTOU in handle_send_message — double mutex acquisition; resolve and send in single lock acquisition
Status: **FIXED**

Evidence:
- `src/mcp/tools.rs:444-470`
  ```rust
  let mut st = lock_state!(state);
  let to_pane = match to_explicit {
      Some(id) => id,
      None => match st.resolve_partner(pane_id) {
          Some(partner) => partner,
          None => return CallToolResult::failure("No partner pane found."),
      },
  };

  if !st.send_message(pane_id, to_pane, content_json, priority) {
  ```

Resolution and send happen under the same lock.

## T-861 — HIGH
Ticket: Fix messages delivered to unregistered pane IDs — no existence check creates phantom inboxes via entry().or_default()
Status: **FIXED**

Evidence:
- `src/mcp/state.rs:202-204`
  ```rust
  if !self.known_panes.contains(&to_pane) {
      return false;
  }
  ```
- Only after that does it call `self.inboxes.entry(to_pane).or_default()`.

## T-862 — HIGH
Ticket: Fix inject_into_pane index vs ID confusion — API mixes pane_index: usize and pane_id: u64 with no type-level distinction
Status: **STILL OPEN**

Evidence:
- `src/main.rs:1066-1075` resolves `req.to_pane` (a `u64` pane ID) to `target_index`, then calls:
  ```rust
  crate::mcp::injection::inject_into_pane(&mut state.panes, idx, &notification)
  ```
- The API still accepts a `usize` index instead of a pane-id typed parameter, so the confusion remains in the interface.

## T-863 — HIGH
Ticket: Fix double raw-mode setup/teardown — both TerminalGuard and ratatui::init set raw mode causing screen flicker
Status: **STILL OPEN**

Evidence:
- `src/main.rs` `TerminalGuard::enter()` still does:
  ```rust
  enable_raw_mode()?;
  execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
  ```
- The project also uses ratatui terminal initialization, so the duplicated ownership of terminal mode is still present.

## T-864 — HIGH
Ticket: Fix _dirty_rx immediately dropped — dirty notifications inoperative, all dirty_tx.send() calls fail silently
Status: **STILL OPEN**

Evidence:
- Current code still keeps the receiver as `_dirty_rx`, indicating it is intentionally unused/dropped rather than driving any dirty-state logic.
- As a result, any `dirty_tx.send()` notifications have no active consumer.

## T-865 — HIGH
Ticket: Fix PENDING_ENTERS uses unstable pane index — pane indices shift on close; store pane_id and look up index dynamically
Status: **FIXED**

Evidence:
- `src/main.rs:1083-1086` stores:
  ```rust
  pending.push(crate::mcp::injection::PendingEnter {
      pane_id: req.to_pane,
      written_at_tick: current_tick,
  ```
- `src/main.rs:1112-1114` re-resolves the pane dynamically:
  ```rust
  if let Some(idx) = state.panes.find_by_pane_id(p.pane_id) {
      if let Some(pane) = state.panes.get_mut(idx) {
  ```

## T-866 — HIGH
Ticket: Fix byte-indexed string slicing panics on multi-byte UTF-8 — 4 locations across agent_picker/dashboard/session; use consistent safe truncate_str
Status: **FIXED**

Evidence:
- Session input rendering in `src/ui/screens/session.rs` converts char index to byte offset with `char_indices()` before slicing.
- Agent-picker/dashboard/session truncation paths now route through `crate::util::truncate_str(...)` rather than raw byte slicing.

## T-867 — HIGH
Ticket: Fix detect() called twice per agent in build_agent_rows — TOCTOU + wasted work; call once and reuse result
Status: **FIXED**

Evidence:
- `src/ui/overlays/agent_picker.rs:142-144`
  ```rust
  let claude_path = claude.detect();
  let codex_path = codex.detect();
  let opencode_path = opencode.detect();
  ```
- Those cached values are then reused for availability and displayed path.

## T-868 — MEDIUM
Ticket: Fix PTY stderr reader ignores kill signal — leaks background task after pane close
Status: **STILL OPEN**

Evidence:
- In `src/pty/mod.rs`, the stderr reader task is still a background task that reads stderr independently; it does not subscribe to a kill/watch shutdown signal the way this ticket calls for.
- I did not find a kill-aware branch that terminates that task early on pane close.

## T-869 — MEDIUM
Ticket: Fix KeybindConfig stores raw strings with no validation — typos silently produce non-functional bindings
Status: **STILL OPEN**

Evidence:
- Keybind handling remains string-driven, and I found no central validation pass that rejects malformed bindings at config load time.
- The failure mode is still “bad string => binding simply doesn’t work.”

## T-870 — MEDIUM
Ticket: Fix wrong JSON-RPC error code for parse errors — uses -32602 (INVALID_PARAMS) instead of -32700 (PARSE_ERROR)
Status: **FIXED**

Evidence:
- `src/mcp/server.rs:48-50`
  ```rust
  let resp = JsonRpcResponse::error(
      Value::Null,
      JsonRpcError::new(PARSE_ERROR, format!("Parse error: {e}")),
  );
  ```
- `PARSE_ERROR` is imported from protocol and defined as `-32700`.

## T-871 — MEDIUM
Ticket: Fix injection fires on MCP tool call failure — missing success check before injecting response
Status: **FIXED**

Evidence:
- `src/mcp/bridge.rs` computes:
  ```rust
  let is_success = serde_json::from_str::<serde_json::Value>(&rpc_response)
      .map(|v| v.get("error").is_none())
      .unwrap_or(false);
  if is_send_message && is_success {
  ```

Injection is now gated on successful RPC result.

## T-872 — MEDIUM
Ticket: Fix RPC request JSON parsed 3× separately — parse once and pass result through
Status: **FIXED**

Evidence:
- `src/mcp/bridge.rs` pre-parses once:
  ```rust
  let parsed_rpc: Option<serde_json::Value> = serde_json::from_str(&bridge_req.request).ok();
  ```
- That same value is reused for `is_send_message_call(...)` and `build_inject_request(...)`.

## T-873 — MEDIUM
Ticket: Fix resolve_partner non-deterministic with >2 panes — HashMap iteration order not guaranteed
Status: **FIXED**

Evidence:
- `src/mcp/state.rs` now stores live pane IDs in `known_panes: Vec<u64>`.
- `resolve_partner()` is:
  ```rust
  self.known_panes.iter().find(|&&id| id != pane_id).copied()
  ```

That removes `HashMap` iteration-order dependence.

## T-874 — MEDIUM
Ticket: Fix TextDone can overwrite wrong turn's content — event may arrive after new turn starts
Status: **STILL OPEN**

Evidence:
- Current reducer logic still updates the most recent assistant transcript slot for `TextDone` rather than matching the event to a turn-specific identifier.
- I did not find a turn ID / sequence guard preventing a late `TextDone` from attaching to the wrong active turn.

## T-875 — MEDIUM
Ticket: Fix UUID session row orphaned — original row unreferenced when SessionBound replaces session_id
Status: **STILL OPEN**

Evidence:
- The current flow still allows a temporary/generated session id to exist before a later `SessionBound` replaces the in-memory `session_id`.
- I did not find persistence-layer migration/remap code that moves or merges the earlier stored row/events onto the bound ID.

## T-876 — MEDIUM
Ticket: Fix MAX_AGENTS = 2 blocks third agent — comment says 3 but constant only allows indices 0-1, blocking OpenCode
Status: **OBSOLETE**

Evidence:
- I searched the current codebase and found no `MAX_AGENTS` constant or equivalent hard-coded two-agent gate.
- The current agent-picker/open flow is structured differently; the exact buggy pattern described by the ticket no longer exists.

## T-877 — MEDIUM
Ticket: Fix input cursor byte-index breaks with multi-byte chars — cursor position by byte offset not character offset
Status: **FIXED**

Evidence:
- `src/input/text_input.rs` updates `session.input_cursor` using `.chars().count()`.
- `src/ui/screens/session.rs` converts the char position safely before slicing:
  ```rust
  let cursor_byte = buf
      .char_indices()
      .nth(cursor_chars)
      .map(|(i, _)| i)
      .unwrap_or(buf.len());
  let before = &buf[..cursor_byte];
  let after = &buf[cursor_byte..];
  ```

## T-878 — MEDIUM
Ticket: Fix in_code_block state persists across transcript entries — unclosed fence bleeds styling into subsequent messages
Status: **FIXED**

Evidence:
- `src/ui/panels/chat.rs` initializes `let mut in_code_block = false;` inside the loop for each transcript entry.
- That resets code-block state at the beginning of every entry, preventing bleed across entries.

## T-879 — MEDIUM
Ticket: Fix hardcoded visible_height in HelpOverlay and FilePreviewPanel — wrong scroll behavior on non-standard terminal sizes
Status: **FIXED**

Evidence:
- `src/ui/overlays/help.rs` stores measured height during render with `self.visible_height.set(visible);` and uses that cached value in key handling.
- `src/ui/panels/file_preview.rs` stores `last_visible_height` from the rendered `inner.height` and reuses it during scrolling.

Note: `HelpOverlay::default()` still seeds `24`, but during normal rendered use it is replaced by actual frame height.

## T-880 — LOW
Ticket: Deduplicate truncate_str implementations — 3 copies in claude_log/codex_log/session/discovery with inconsistent semantics
Status: **STILL OPEN**

Evidence:
- `rg` still finds `truncate_str` implementations/usages in:
  - `src/claude_log.rs`
  - `src/codex_log.rs`
  - `src/session/discovery.rs`
  - plus shared `src/util.rs`

The duplication has not been fully removed.

## T-881 — LOW
Ticket: Fix compact_json byte/char truncation confusion — mixes byte and character semantics in claude_log.rs
Status: **STILL OPEN**

Evidence:
- `src/claude_log.rs` `compact_json(...)` still truncates serialized JSON using a byte slice (`s[..157]` style logic) rather than character-safe slicing.
- That preserves the byte/char semantic mismatch described in the ticket.

## T-882 — LOW
Ticket: Fix get_partner_status silently omits roleless panes — panes without claimed roles excluded from response
Status: **FIXED**

Evidence:
- `src/mcp/state.rs` `get_partner_status()` iterates `known_panes` and builds a default role when none exists:
  ```rust
  role: self.roles.get(id).cloned().unwrap_or(PaneRole {
      name: "unassigned".to_string(),
      description: String::new(),
  }),
  ```

Roleless panes are now included.

## T-883 — LOW
Ticket: Fix initialized notification returns spurious success response — MCP initialized is a notification, no response expected
Status: **FIXED**

Evidence:
- `src/mcp/server.rs`:
  ```rust
  "initialized" => {
      // Notification — per JSON-RPC 2.0, no response should be sent.
      return String::new();
  }
  ```

## T-884 — LOW
Ticket: Fix tool_result.content only handles string form — MCP spec allows array of content blocks; only string shorthand handled
Status: **STILL OPEN**

Evidence:
- Current Claude tool-result handling is still string-oriented. The adapter/tests reference inputs like:
  ```rust
  {"type":"tool_result","tool_use_id":"t1","content":"file contents here"}
  ```
- I did not find parsing that walks array content blocks and normalizes them into a preview/result payload.

## T-885 — LOW
Ticket: Remove unused pane_index_after_open computation — computed but never read in main.rs
Status: **OBSOLETE**

Evidence:
- I searched the current `src/main.rs` and the wider repo; `pane_index_after_open` is gone.
- The exact dead computation described by the ticket no longer exists.

## T-886 — LOW
Ticket: Fix duplicate keybind entries in help overlay — same keybinds listed more than once
Status: **STILL OPEN**

Evidence:
- `src/ui/overlays/help.rs` still repeats bindings across sections, for example:
  - `Tab` appears in Global, Terminal, and Navigation
  - `Shift+Tab` appears in Global and Navigation
  - `Ctrl+Q` appears in Global and Terminal

The duplicate listing remains.

## T-887 — LOW
Ticket: Fix TokenSparkline::push uses O(n) Vec::remove(0) — should use VecDeque for O(1) pop_front
Status: **FIXED**

Evidence:
- `src/ui/widgets/sparkline.rs` now imports and stores `VecDeque`:
  ```rust
  use std::collections::VecDeque;
  pub struct TokenSparkline {
      pub data: VecDeque<u64>,
  }
  ```
- The tests assert deque behavior (`VecDeque::from(vec![2, 3, 4])`), confirming the O(1) front-pop structure is in place.
