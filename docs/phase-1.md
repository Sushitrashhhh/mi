# Phase 1: Agent loop, tools, permissions

## Built

- **Tool registry** (`tools/registry.rs`): `Tier` enum (READ/SAFE/CONFIRM/BLOCKED), `ToolDef`
  with JSON schema parameters, `ToolResult`, `ToolRegistry` with Gemini function declaration
  generation, `render_summary` for human-readable tool action descriptions.

- **Permission engine** (`permissions/policy.rs`): `PolicyEngine` with `check()` that returns
  Auto/Ask/Refuse based on tier, taint state, shell allowlist, and always-approve rules.
  Taint escalation: after untrusted content enters context (files, web), SAFE tools are
  bumped to CONFIRM. Shell commands always require CONFIRM unless allowlisted in `policy.toml`.

- **Audit log** (`permissions/audit.rs`): append-only JSONL log at `~/.mi/logs/audit.jsonl`.
  Logs every LLM call, tool execution, and approval decision with timestamps and session IDs.

- **OS adapter** (`tools/os_adapt/`): `OsAdapter` trait with async methods for `open_url`,
  `open_app`, `list_windows`, `focus_window`, `close_app`, `type_text`, `send_hotkey`,
  `system_info`. Windows implementation uses PowerShell via `tokio::process::Command`.

- **Tool packs** (`tools/builtin/`):
  - `system` — time/date (READ), system info (READ)
  - `files` — list (READ), read (READ, taints), create (SAFE), move (CONFIRM), delete/quarantine (CONFIRM)
  - `apps` — list (READ), open app/URL (SAFE), focus (SAFE), close (CONFIRM), type text (CONFIRM), hotkey (CONFIRM)
  - `shell` — run command (CONFIRM, with allowlist support)
  - `web` — search/fetch (READ, taints) — placeholder, returns "not yet implemented"
  - `timers` — set (SAFE), list (READ), cancel (SAFE) — in-memory

- **Agent loop** (`agent/loop_runner.rs`): plan→act→observe loop with Gemini function calling.
  Streams LLM responses, executes tool calls per policy, requests approval for CONFIRM tools,
  feeds results back, loops until final text answer or max_steps (default 12).

- **LLM provider abstraction** (`llm/mod.rs`): `AnyProvider` enum wrapping `GeminiProvider` and
  `FakeProvider` for testability.

- **CLI** updated:
  - `mi ask "..."` now uses the agent loop with tool calling and inline approval prompts
  - `mi chat` supports tools and approvals across multi-turn conversations
  - `mi logs [--tail N]` shows the audit log
  - `mi doctor` now reports the number of registered tools

- **Quarantine**: `files.delete` moves files to `~/.mi/quarantine/` with timestamps, never
  permanently deletes.

## Numbers

- **Tests**: 33 pass (up from 14), clippy clean
- **New files**: 16 source files across tools/, permissions/, agent/
- **Tools registered**: 20 (system: 2, files: 5, apps: 6, shell: 1, web: 2, timers: 3)

## Try it

```
# Ask something that uses tools
mi ask "what time is it?"
mi ask "open Chrome and search for lo-fi beats"
mi ask "list files in my Downloads folder"

# This should ask for approval:
mi ask "delete old.txt"

# Interactive chat with tools
mi chat

# View audit log
mi logs --tail 10

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings
```

## Acceptance criteria

- ✅ "Open Chrome and search for lo-fi beats" works (apps.open with URL, SAFE tier auto-executes)
- ✅ "Delete old.txt" asks first (files.delete is CONFIRM tier, CLI prompts user)
- ✅ Injection test passes: web content cannot trigger deletion without approval (test_injection_cannot_trigger_deletion)
- ✅ Taint escalation test passes: SAFE tools escalated to CONFIRM after untrusted content (test_taint_escalates_safe_to_confirm)
- ✅ Audit log shows everything (tool calls, approvals, LLM calls logged to ~/.mi/logs/audit.jsonl)
- ✅ 33 tests pass, clippy clean

## Known gaps

- WebSocket server still uses direct Gemini streaming (not agent loop) — needs update for overlay
- `mi approve` CLI command not yet implemented (approvals are inline in ask/chat)
- `web.search` and `web.fetch` are placeholders (Phase 6+ integration)
- Timers are in-memory only, don't survive restarts (Phase 6 for persistent scheduling)
- No overlay/tray yet (Phase 3)
- No memory/context yet (Phase 2)
