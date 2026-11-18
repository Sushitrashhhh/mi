# Phase 0: Skeleton & text brain

## Built
- `core/` uv project (Python 3.12), package `mi_core`, entry point `mi`.
- `config.py`: `~/.mi/mi.toml` (created on first run, embedded default), pydantic-validated.
  Host is locked to `127.0.0.1`. `MI_HOME` env var relocates the data dir (used by tests).
- `secrets.py`: Gemini key in the OS keyring. `GEMINI_API_KEY` env var works as a dev-only fallback.
- `events.py`: event protocol envelope + all payload models from BLUEPRINT 5.1.
- `server/`: FastAPI WebSocket at `ws://127.0.0.1:7777/ws?token=...`; token in `~/.mi/token`.
  Handles `user.message` → streams `assistant.delta` / `assistant.done`.
- `llm/`: `LLMProvider` protocol, `GeminiProvider` (streaming + `models.list()`), `FakeProvider` for tests.
- `doctor.py` + `mi doctor [--fix] [--json]`: checks key and every configured model ID against
  `client.models.list()`. Suggestions come from the live list, never from hardcoded IDs.
- CLI: `mi ask`, `mi chat`, `mi serve`, `mi doctor`, `mi auth set-gemini-key`, `mi version`.
- `scripts/build.ps1`: Nuitka one-file build → `dist/mi.exe` (uses Nuitka's own MinGW64).

## Try it
```
cd core
uv run mi auth set-gemini-key     # paste key at the hidden prompt
uv run mi doctor --fix            # fills [models] in ~/.mi/mi.toml from your key's model list
uv run mi doctor
uv run mi ask "hello"
uv run mi chat
uv run pytest
```
Build the exe: `./scripts/build.ps1` → `dist/mi.exe`, then `dist\mi.exe ask "hello"`.

## Known gaps
- `mi ask`/`chat` call Gemini directly; routing through a running service comes with the agent loop (Phase 1/3).
- `mi` with no args runs the server for now; tray + overlay arrive in Phase 3.
- Model suggestions in `doctor --fix` use name-family heuristics (flash-lite / flash / pro / tts / embed)
  over the live list. Check the result and edit `mi.toml` if you prefer different models.
- Live Gemini calls are untested by CI (no key); covered by mocked-SDK tests.
