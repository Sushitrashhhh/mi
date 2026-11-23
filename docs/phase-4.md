# Phase 4: Voice

## What was built

### Audio capture (`mi-core/voice/capture.rs`)
- Opens default input device via `cpal` 0.18
- Handles F32, I16, I32 sample formats with stereo→mono conversion
- Linear interpolation resampling to 16 kHz (whisper's native rate)
- `AudioCapture::start_recording()` → `RecordingHandle` for async buffering
- `RecordingHandle::finish()` returns final `Vec<f32>` at 16 kHz mono

### Voice Activity Detection (`mi-core/voice/vad.rs`)
- Energy-based VAD using RMS threshold (configurable `vad_threshold`)
- Three events: `Speech`, `Silence`, `SilenceTimeout`
- Configurable silence timeout (default 8 s)

### Local STT via whisper.cpp (`mi-core/voice/stt.rs`)
- Rust bindings via `whisper-rs` 0.16 (whisper.cpp under the hood)
- Fully local, free, zero-cost — no cloud API calls
- Greedy decoding, single-segment, English, up to 4 threads
- GGML model files stored in `~/.mi/models/`

### Model management (`mi-core/voice/models.rs`)
- Downloads whisper GGML models from HuggingFace on first use
- Atomic writes (`.part` file → rename) to avoid corruption
- Supports any model size: tiny, base, small, medium, large
- Default: `tiny` (~75 MB, fastest)

### Text-to-speech (`mi-core/voice/tts.rs`, `tts_kelados.rs`)
Two selectable backends (`voice.tts_backend` in config), both local and free:
- **Kelados / Kokoro** (`tts_kelados.rs`, default): HTTP client (`reqwest`) to a local
  Kelados server (`npx kelados serve`, default `127.0.0.1:8880`) exposing an
  OpenAI-compatible `POST /v1/audio/speech`. Returns 24 kHz WAV. Natural Kokoro
  voices (default `af_heart`, a Grade-A female voice). If the server is unreachable,
  the pipeline **falls back to Windows TTS** automatically.
- **Windows native** (`tts.rs`): `windows` 0.62 WinRT `SpeechSynthesizer`, prefers a
  female voice (Zira/Aria). Synthesizes to WAV via `SynthesizeTextToStreamAsync`,
  blocking on WinRT async with `futures::executor::block_on`. Non-Windows stub errors
  gracefully.

### Voice pipeline (`mi-core/voice/pipeline.rs`)
- Orchestrates: Mic → VAD → Whisper STT → Agent Loop → Windows TTS → Playback
- Lazy model download: `ensure_stt()` downloads on first voice use
- CPU-heavy work (STT, TTS) runs in `tokio::task::spawn_blocking`
- State machine: Idle → Listening → Transcribing → Speaking → Idle

### Audio playback (`mi-bin/main.rs`)
- Uses `rodio` 0.23 with `DeviceSinkBuilder` and `Player`
- `play_wav_bytes()` runs in `spawn_blocking` to avoid blocking async runtime

### PTT hotkey (`mi-bin/overlay/hotkeys.rs`)
- **Ctrl+Space** — press to start listening, release to transcribe and send
- Registered alongside existing Ctrl+Shift+M (type mode) and Ctrl+Alt+Esc (kill switch)

### Overlay voice UI (`mi-bin/overlay/app.rs`)
- Three new phases: `Listening`, `Transcribing`, `Speaking`
- **Listening**: animated audio bars (5 red bars with sine-wave heights)
- **Transcribing**: pulsing amber dots animation
- **Speaking**: pulsing green circles animation
- Status dot in header changes color for voice phases
- Footer hints update: "Ctrl+Space to talk" / "release Ctrl+Space to send"
- Kill switch cancels Speaking phase too

### Backend integration (`mi-bin/main.rs`)
- Voice pipeline initialized on startup, wrapped in `Arc<Mutex>`
- Internal `BackendMsg` enum: `Text(String)`, `PttStart`, `PttStop`
- PttStart: ensures STT ready (downloads model if needed), starts mic capture
- PttStop: transcribes audio, runs agent loop, speaks response, plays WAV

### Configuration (`mi-core/config.rs`, `config/mi.example.toml`)
```toml
[voice]
pipeline = "pipeline"       # "pipeline" (PTT) or "live" (future streaming)
stt_model = "tiny"          # whisper model size
silence_timeout_s = 8.0
vad_threshold = 0.02
barge_in = false            # future: interrupt TTS with new speech
```

## How to run

```sh
# Start the assistant (overlay + voice)
cargo run -p mi-bin

# Build requirements:
#   - CMake (for whisper.cpp compilation)
#   - LLVM/libclang (for whisper-rs-sys bindgen)
# Set env vars if not in PATH:
#   LIBCLANG_PATH="C:\Program Files\LLVM\bin"

# Voice usage:
#   1. Hold Ctrl+Space to start listening
#   2. Speak your query
#   3. Release Ctrl+Space — mi transcribes and responds
#   4. Hear the spoken answer via Windows TTS
# The whisper model (~75 MB for tiny) downloads automatically on first use.

# Tests
cargo test

# Lint
cargo clippy -- -D warnings
```

## Dependencies added

### mi-core
- `cpal` 0.18 — cross-platform audio capture
- `whisper-rs` 0.16 — local STT via whisper.cpp
- `windows` 0.62 (features: `Media_SpeechSynthesis`, `Media_Core`, `Foundation`, `Storage_Streams`) — Windows TTS

### mi-bin
- `rodio` 0.23 — audio playback

### Build-time requirements
- CMake (for whisper.cpp C compilation)
- LLVM with libclang.dll (for whisper-rs-sys bindgen)

## Wake word ("hey mi") — added after Phase 5
- Always-on, fully local keyword spotting that **reuses the loaded whisper model**
  (no new account, API, or dependency — fits the zero-cost/local rule).
- `voice/wake.rs` — `WakeDetector`: normalizes a transcript and scores it against
  the phrase with **word-structured** fuzzy matching (per-word similarity over
  sliding windows), so unrelated speech stays well below threshold.
- `voice/capture.rs` — `start_continuous(secs)` ring buffer + `recent_16k()` for
  non-destructive polling.
- `pipeline.rs` — `poll_wake()` is **energy-gated**: whisper only runs when the mic
  window actually has speech, so idle CPU stays near zero.
- `main.rs` — a background wake loop (guarded by `VoiceState` and a TTS-active flag
  so mi never wakes itself) triggers a hands-free command turn: listen until VAD
  silence → transcribe → agent → spoken reply.
- Config: `voice.wake_enabled` (default true), `voice.wake_word` ("hey mi"),
  `voice.wake_threshold` (0.6). `silence_timeout_s` lowered to 2.0 for snappy
  hands-free auto-stop.
- The same hands-free listen path also powers Ctrl+Shift+S screenshot-ask.

## Known gaps
- Barge-in not yet implemented (config exists, logic pending)
- Live/streaming mode not yet implemented (config exists, pipeline only)
- Spoken approval flow not yet implemented ("Send… commands ask for a spoken yes")
- Wake accuracy depends on whisper `tiny`; false-wake rate is tunable via `wake_threshold`
- No model selection UI — change `stt_model` in `~/.mi/mi.toml`
- Latency target (<2 s) depends on hardware and whisper model size
