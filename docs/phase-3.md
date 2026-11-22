# Phase 3: System Presence & Overlay

## What was built

### System tray icon (`overlay/tray.rs`)
- Programmatic cornflower-blue circle icon (32x32 with anti-aliased edges)
- Context menu: "Talk to mi" (opens overlay) and "Quit" (shuts down)
- Uses `tray-icon` 0.26 + `muda` 0.21

### Global hotkeys (`overlay/hotkeys.rs`)
- **Ctrl+Shift+M** — toggle type mode (opens the overlay bar)
- **Ctrl+Alt+Esc** — kill switch (cancels running agent turn)
- Uses `global-hotkey` 0.10, events polled on the main thread in `logic()`

### Egui overlay bar (`overlay/app.rs`)
- Borderless, always-on-top, non-resizable window using eframe 0.37
- Dark theme with indigo accent, custom color tokens (BG_DARK, ACCENT, etc.)
- Phase-based UI: Hidden → Input → Thinking → Responding → Approval → Done
- Styled text input with rounded frame, placeholder text, accent focus ring
- Animated thinking indicator (pulsing dots)
- Tool chips with status colors (amber=running, green=ok, red=fail)
- Response area in a card with scroll
- Approval card: amber-bordered, collapsible args detail, Approve/Deny/Always buttons
- Shadow and rounded corners on the main bar
- Keyboard hints in the footer that change per phase
- Auto-hides 5 seconds after completion
- Centered on screen, force-hidden on first frame to avoid flash
- Status dot in header changes color by phase

### Kill switch (cancel token)
- `CancellationToken` from `tokio-util` integrated into `AgentLoop`
- Checked at top of step loop and between streaming chunks
- Kill switch cancellation propagates immediately
- New `AgentError::Cancelled` variant

### Single-instance lock (`instance_lock.rs`)
- TCP listener on `127.0.0.1:17776` prevents multiple instances
- PID file at `~/.mi/mi.pid` for diagnostics
- Cleaned up on drop

### Autostart (`service.rs`)
- `mi install-service` — creates Windows Task Scheduler entry (ONLOGON, LIMITED)
- `mi uninstall-service` — removes the scheduled task
- Task name: `MiAssistant`

### Native notifications (`overlay/notify.rs`)
- Toast notifications for errors and approval requests via `notify-rust`
- Fires when overlay may be hidden, so user knows mi needs attention

### Architecture
- **Main thread**: runs eframe (blocking) — hosts the egui overlay, polls tray and hotkey events
- **Backend thread**: runs its own tokio runtime — processes messages, runs agent loop
- **Communication**: `std::sync::mpsc` channels for UI ↔ backend, `tokio::sync::oneshot` for approval flow
- **Context bridge**: egui `Context` sent from main to backend via `SyncSender` so backend can trigger repaints

### Updated `main.rs`
- `mi` (no args) or `mi start` launches the full overlay + tray + backend
- `mi install-service` / `mi uninstall-service` for autostart management

## How to run

```sh
# Start the assistant (tray + overlay)
cargo run -p mi-bin

# Or explicitly
cargo run -p mi-bin -- start

# Register to start at login
cargo run -p mi-bin -- install-service

# Remove from login
cargo run -p mi-bin -- uninstall-service

# Build release
cargo build --release

# Check + lint
cargo clippy -- -D warnings
```

## Dependencies added (mi-bin)
- `eframe` 0.37 — egui native overlay
- `tray-icon` 0.26 — system tray icon
- `muda` 0.21 — tray context menu
- `global-hotkey` 0.10 — global hotkey registration
- `notify-rust` 4 — native OS notifications
- `image` 0.25 — programmatic icon creation
- `chrono` 0.4 — timestamps
- `tokio-util` 0.7 — CancellationToken (also in mi-core)
- `dirs` 6 — home directory for PID file

### Gemini thought_signature round-trip (`mi-core/llm`)
- Newer Gemini models attach `thoughtSignature` to functionCall parts
- Must be echoed back in assistant Content or the API returns an error
- Added `thought_signature: Option<String>` to `FunctionCall` and `Part`
- Captured from response parts, echoed when constructing assistant Content

## Known gaps
- Voice mode not yet implemented (Phase 4)
- Wake word not yet implemented (Phase 4)
- Overlay animation/slide-up transition not implemented (cosmetic)
- No conversation persistence across overlay sessions (each message is stateless)
- Bar doesn't prevent focus stealing in all edge cases (Windows compositor limits)
