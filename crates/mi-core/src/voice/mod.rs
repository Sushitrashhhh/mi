pub mod capture;
pub mod models;
pub mod pipeline;
pub mod stt;
pub mod tts;
pub mod vad;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("no input device available")]
    NoInputDevice,
    #[error("no output device available")]
    NoOutputDevice,
    #[error("audio stream error: {0}")]
    AudioStream(String),
    #[error("STT model not found at {0} — run `mi doctor --fix` to download")]
    ModelNotFound(String),
    #[error("STT transcription failed: {0}")]
    Transcription(String),
    #[error("TTS synthesis failed: {0}")]
    Synthesis(String),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("voice pipeline not initialized")]
    NotInitialized,
}
