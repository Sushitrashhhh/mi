# Phase 2: Memory

## Built

- **Memory store** (`memory/store.rs`): SQLite-backed persistence for memories, conversations,
  messages, and working context. Schema matches BLUEPRINT §5.4. FTS5 for full-text search with
  LIKE fallback. Embedding storage as BLOBs with a custom `cosine_similarity` SQL function
  registered via rusqlite (no sqlite-vec extension needed).

- **Embedding client** (`memory/embed.rs`): Gemini `embedContent` REST API client via reqwest.
  Supports single and batch embedding. Model and dimensions configurable — defaults to
  `gemini-embedding-2` with 768 dimensions (via `outputDimensionality`). Never hardcodes model IDs.

- **Hybrid retrieval** (`memory/retrieve.rs`): combines vector similarity search + FTS keyword
  search using Reciprocal Rank Fusion (RRF). Pinned memories always included. Returns top-k
  relevant memories for context injection.

- **Memory extractor** (`memory/extractor.rs`): post-turn LLM call extracts candidate memories
  from conversation. Uses the `fast` model (cheapest configured model). Respects `auto_save`
  config: `off`, `stated_only` (conservative), `all` (aggressive). Secret filtering via regex
  strips API keys, tokens, credit cards, SSNs, and passwords before storage.

- **Config** (`config.rs`): `[memory]` section with `top_k` (default 6), `auto_save`
  (default `stated_only`), `embed_dim` (default 768, read from config per BLUEPRINT rules).

- **Agent loop integration** (`agent/loop_runner.rs`): retrieves relevant memories and injects
  them into the system prompt before each turn. Runs extraction asynchronously after each
  completed turn. Both are best-effort — failures log warnings, don't break the conversation.

- **CLI** (`mi memory ...`):
  - `mi memory list [--limit N]` — list memories with kind, text, pin status
  - `mi memory search <query>` — FTS keyword search
  - `mi memory forget <id>` — soft-delete (supports 8-char prefix IDs)
  - `mi memory pin <id> [--unpin]` — pin/unpin a memory
  - `mi memory export [--output file.json]` — export all as JSON
  - `mi memory wipe` — delete everything (requires confirmation)

- **System prompt** updated with memory awareness — mi knows it has persistent memory and uses
  recalled facts naturally without quoting them back.

- **Doctor** updated: `mi doctor` now shows memory DB status and count.

## Numbers

- **Tests**: 59 pass (up from 33), clippy clean
- **New files**: 5 source files in `memory/`
- **Dependencies added**: `rusqlite` 0.43 (bundled + functions), `regex` 1.11

## Try it

```
# Start a chat and tell mi something about yourself
mi chat
> I edit videos in Premiere Pro and I like short answers

# Check that mi remembers (after restart)
mi memory list
mi memory search "Premiere"

# Forget a memory
mi memory forget <id-prefix>

# Export all memories
mi memory export --output memories.json

# Wipe everything
mi memory wipe

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings
```

## Acceptance criteria

- ✅ Tell mi "I edit in Premiere and like short answers" — memories extracted and stored in SQLite
- ✅ After restart, relevant memories injected into system prompt via hybrid retrieval
- ✅ `forget` works — soft-deletes from memories, FTS, and embeddings; never resurfaces
- ✅ Secrets filtered — API keys, tokens, credit cards, SSNs, passwords stripped before storage (7 tests)
- ✅ 59 tests pass, clippy clean

## Known gaps

- Embedding requires a live Gemini API key — retrieval falls back to text-only search when
  embedding fails (graceful degradation, logged warning)
- Rolling conversation summaries (episodes) not yet auto-generated — the schema and plumbing
  exist but the summarization LLM call is deferred to avoid complexity
- Working context (active window, OS state) not auto-updated yet — requires OS hooks from Phase 3
- `mi memory` CLI doesn't yet support semantic (vector) search — only FTS keyword search;
  the full hybrid retrieval runs automatically during conversation via the agent loop
