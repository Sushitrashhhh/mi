# Phase 5: Vision

## What was built

### Multimodal LLM layer (`mi-core/llm`)
- `Message` now carries `images: Vec<ImageData>` (`types.rs`); `ImageData { mime_type, data }` where `data` is base64.
- `Message::user_with_images(text, images)` constructor.
- Gemini provider (`gemini.rs`) emits images as `inlineData` parts (`{mimeType, data}`) on user messages, matching the v1beta REST image format. Added `Part::text()` / `Part::image()` helpers.

### Screen capture (`mi-core/vision/`)
- `capture.rs` uses `xcap` 0.9 to grab the primary monitor or the active window, encodes to PNG **in memory** (never written to disk), and base64-encodes for the model. `CaptureTarget::{PrimaryMonitor, ActiveWindow}`.
- `Screenshot { png, width, height }` with `to_base64()` / `mime_type()`.
- Display-gated test (`--ignored`) verifies a real capture produces a valid PNG.

### `screen.capture` tool (`mi-core/tools/builtin/screen.rs`)
- READ tier, **taints the turn** (screen content is untrusted → escalates later SAFE tools to CONFIRM).
- Optional `target` arg: `"screen"` (default) or `"window"`.
- Returns `ToolResult` carrying the captured image. The agent loop appends it as a follow-up user message so the model can see it.
- Runs the blocking capture in `spawn_blocking`.

### Agent loop image injection (`mi-core/agent/loop_runner.rs`)
- Collects images from tool results; after the tool-response message, appends a `user_with_images` message so the model receives the screenshot on the next step.

### Privacy config (`mi-core/config.rs`, `config/mi.example.toml`)
```toml
[privacy]
keep_screenshots = false       # in-memory only by default
redact_screenshots = true      # reserved; redaction deferred
retention_days = 90
```
- `MiConfig::active_vision_model()` prefers `models.vision`, falling back to the default model (modern Gemini models are multimodal).

### Overlay & hotkeys (`mi-bin`)
- **Ctrl+Shift+S** (`hotkeys.rs`): capture the primary monitor *before* the overlay appears, then start push-to-talk listening. The transcribed question is sent with the screenshot attached.
- **Image drop** (`app.rs` `poll_dropped_images`): drop a PNG/JPEG/WebP/GIF onto the overlay to attach it to the next typed or spoken message.
- `UiCommand::{ScreenshotAsk, AttachImage}` and `BackendMsg::{ScreenshotAsk, AttachImage}` route these through the backend.
- `main.rs` holds a `pending_image` that attaches to the next PTT or typed turn and routes that turn to the vision model.

## How it works end to end
- **Ask by voice/text:** "capture my screen and explain the error" → the model calls `screen.capture` → the image is injected → the model explains the error.
- **Ctrl+Shift+S:** capture now, then speak your question hands-on-key.
- **Drag an image** onto the overlay, then type/speak about it.

Acceptance: *"What's this error on my screen?"* explains the visible error. Screenshots never persist on disk (`keep_screenshots = false`).

## Dependencies added (mi-core)
- `xcap` 0.9 — cross-platform screen/window capture (Windows 8.1+)
- `image` 0.25 (`default-features = false`, `features = ["png"]`) — PNG encode
- `base64` 0.22 — encode image bytes for the Gemini `inline_data` part

mi-bin also adds `base64` 0.22 for dropped-image encoding.

## How to run
```sh
cargo run -p mi-bin
# Hold Ctrl+Space and say "capture my screen and tell me what's wrong", or
# press Ctrl+Shift+S then speak, or drag an image onto the overlay.

cargo test                                   # 69 pass (+ 1 ignored capture test)
cargo test -p mi-core vision -- --ignored    # runs the real screen-capture test
cargo clippy -- -D warnings
```

## Known gaps
- **Redaction deferred** — `redact_screenshots` is a config no-op for now; the real privacy guarantees (in-memory only + taint) are in place.
- Wake word ("hey mi") still not built — Ctrl+Shift+S / Ctrl+Space are hands-on-key. (Next up, per the Phase 4 follow-up.)
- Multi-monitor: captures the primary monitor; no monitor picker yet.
- No prompt-injection test yet for malicious on-screen text (the taint path is wired, test pending).
