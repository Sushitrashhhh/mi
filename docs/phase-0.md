# Phase 0: Skeleton & text brain (Rust)

## Built

- **Cargo workspace** with two crates:
  - `mi-core`: library with config, secrets, events, LLM (Gemini REST), WebSocket server
  - `mi-bin`: binary crate with clap CLI

- **Config** (`config.rs`): loads `~/.mi/mi.toml` with `[core]` and `[models]` sections.
  Defaults: host 127.0.0.1, port 7777, autonomy normal, max_steps 12.

- **Secrets** (`secrets.rs`): Gemini API key stored in OS keyring (Windows Credential Manager)
  via the `keyring` crate. Tries both `mi-assistant` and legacy `mi-core` service names.

- **Events** (`events.rs`): `Envelope` wrapper with type/id/ts/session/data, plus all payload
  types (UserMessage, AssistantDelta, AssistantDone, ApprovalRequest, etc.) with serde.

- **Gemini REST client** (`llm/gemini.rs`): direct HTTP via reqwest, no SDK.
  - Streaming via SSE (`streamGenerateContent?alt=sse`)
  - `list_models()` via `GET /v1beta/models`
  - Function calling support (tools/functionDeclarations in request, functionCall/functionResponse in messages)
  - Message conversion: user→user, assistant→model, tool→user(functionResponse)

- **FakeProvider** (`llm/fake.rs`): deterministic test provider with ScriptedReply.

- **WebSocket server** (`server/ws.rs`): axum on localhost:7777 with token auth.
  Handles `user.message` → streams Gemini response → emits delta/done events.

- **CLI** (`mi-bin/main.rs`): clap-based, single binary.
  - `mi ask "..."` — one-shot with streaming
  - `mi chat` — interactive REPL with history
  - `mi doctor [--fix]` — validates key, lists models, auto-picks default
  - `mi serve` — starts WebSocket server
  - `mi auth set-gemini-key [KEY]` — stores key in OS keyring
  - `mi version` — prints version

## Numbers

- **Binary size**: 9.3 MB (vs 60-120 MB with Python+Nuitka)
- **Build time**: ~2 min release, ~27s check
- **Tests**: 14 pass, clippy clean
- **Dependencies**: 344 crates (tokio, reqwest, axum, serde, clap, keyring, etc.)

## Try it

```
# Set your Gemini API key
mi auth set-gemini-key YOUR_KEY

# Auto-pick a model
mi doctor --fix

# Ask something
mi ask "hello"

# Interactive chat
mi chat

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings
```

## Acceptance criteria

- ✅ `mi ask "hello"` streams a Gemini reply
- ✅ `mi doctor` lists valid models and flags bad config
- ✅ `cargo build --release` produces a single 9.3 MB `mi.exe`
- ✅ Tests pass (14 tests), clippy clean
- ✅ No key in any file

## Known gaps

- WebSocket server does not yet use the agent loop (Phase 1)
- No tool registry yet (Phase 1)
- No overlay/tray (Phase 3)
