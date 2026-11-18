# Phase 1: Agent loop, tools, permissions

## Built
- **Tool registry** (`tools/registry.py`): `@tool` decorator with name, tier, description, summary.
  Schema generation for Gemini function declarations. `Tier` enum: READ, SAFE, CONFIRM, BLOCKED.
  `ToolResult` with ok/content/artifacts/undo. BLOCKED tools are excluded from declarations.

- **Policy engine** (`permissions/policy.py`): checks tool calls against tier rules.
  - Taint tracking: after untrusted content enters context (file reads, web, shell), SAFE→CONFIRM.
  - `dry_run` mode: all actions need approval. `paused`: all refused.
  - Scheduled runs never auto-approve CONFIRM tools.
  - `policy.toml` allowlist for `approve_always` rules.

- **Agent loop** (`agent/loop.py`): Gemini function calling with multi-step tool use.
  - Streams LLM response, detects function calls, checks policy, executes tools, feeds results back.
  - Max steps limit (default 12). Taint flag propagates across steps.
  - Approval handlers: deny-all (default), auto-approve (CLI), WebSocket (server).
  - Thought signature passthrough for Gemini API compatibility.

- **Audit log** (`agent/audit.py`): append-only JSONL at `~/.mi/logs/audit.jsonl`.
  Logs turn starts, tool calls (with tier/args/ok), approvals, refusals.

- **OS adapter** (`tools/os_adapt/`): `OSAdapter` protocol with Windows implementation.
  Uses subprocess/PowerShell for open_app, list_running, focus_window, close_app,
  send_keys, notify, set_volume, get_battery, run_command.

- **Built-in tool packs** (22 tools):
  - `system`: time, info, battery (READ), volume, notify (SAFE)
  - `files`: list, read, search (READ), create (SAFE), move, delete/quarantine (CONFIRM)
  - `apps`: list_running, active_window (READ), open, focus (SAFE), close (CONFIRM)
  - `shell`: run command (CONFIRM, always)
  - `web`: search, fetch (READ, placeholders — full implementation in later phase)
  - `timers`: set (SAFE), list (READ), cancel (SAFE) — in-memory for now

- **WebSocket server** updated to use agent loop with approval flow.
- **CLI**: `mi ask` and `mi chat` now use the agent loop with tools. `mi logs` shows audit log.

## Try it
```
cd core
uv run mi ask "what time is it?"
uv run mi ask "open Chrome and search for lo-fi beats"
uv run mi ask "list files in C:\Users"
uv run mi chat
uv run mi logs
uv run pytest
```

## Acceptance criteria
- ✅ "Open Chrome and search for lo-fi beats" works with no prompt
- ✅ "Delete old.txt" asks first (CONFIRM tier, denied by default handler)
- ✅ Poisoned web/file content cannot trigger deletion without approval (3 injection tests)
- ✅ Audit log shows everything (`mi logs`)

## Known gaps
- `web.search` and `web.fetch` are placeholders (grounded search comes later).
- Timers are in-memory (lost on restart). Persistent scheduling in Phase 6.
- No overlay approval cards yet (Phase 3). WebSocket approval works for programmatic clients.
- `mi approve` CLI for interactive approval not yet wired (needs running server + WebSocket).
- macOS/Linux OS adapters not implemented.
