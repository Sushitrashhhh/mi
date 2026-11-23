use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::VoiceConfig;

use super::capture::{AudioCapture, RecordingHandle};
use super::models;
use super::stt::WhisperStt;
use super::tts::WindowsTts;
use super::vad::{VadEvent, VoiceActivityDetector};
use super::VoiceError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Transcribing,
    Speaking,
}

pub struct VoicePipeline {
    stt: Option<Arc<WhisperStt>>,
    tts: Option<Arc<WindowsTts>>,
    capture: Option<Arc<AudioCapture>>,
    recording: Arc<Mutex<Option<RecordingHandle>>>,
    vad: Arc<Mutex<VoiceActivityDetector>>,
    config: VoiceConfig,
    state: Arc<Mutex<VoiceState>>,
}

impl VoicePipeline {
    pub fn new(config: &VoiceConfig) -> Self {
        let vad = VoiceActivityDetector::new(config.vad_threshold, config.silence_timeout_s);

        Self {
            stt: None,
            tts: None,
            capture: None,
            recording: Arc::new(Mutex::new(None)),
            vad: Arc::new(Mutex::new(vad)),
            config: config.clone(),
            state: Arc::new(Mutex::new(VoiceState::Idle)),
        }
    }

    pub async fn init(&mut self) -> Result<(), VoiceError> {
        let capture = AudioCapture::new()?;
        self.capture = Some(Arc::new(capture));

        let model_path = models::whisper_model_path(&self.config.stt_model);
        if model_path.exists() {
            let stt = WhisperStt::new(&model_path)?;
            self.stt = Some(Arc::new(stt));
        }

        match WindowsTts::new() {
            Ok(tts) => self.tts = Some(Arc::new(tts)),
            Err(e) => tracing::warn!("TTS init failed (will work without voice output): {}", e),
        }

        Ok(())
    }

    pub async fn ensure_stt(&mut self) -> Result<(), VoiceError> {
        if self.stt.is_some() {
            return Ok(());
        }

        let model_path = models::ensure_whisper_model(&self.config.stt_model).await?;
        let stt = WhisperStt::new(&model_path)?;
        self.stt = Some(Arc::new(stt));
        Ok(())
    }

    pub fn is_stt_ready(&self) -> bool {
        self.stt.is_some()
    }

    pub fn is_tts_ready(&self) -> bool {
        self.tts.is_some()
    }

    pub fn is_model_downloaded(&self) -> bool {
        models::is_model_available(&self.config.stt_model)
    }

    pub async fn state(&self) -> VoiceState {
        self.state.lock().await.clone()
    }

    pub async fn start_listening(&self) -> Result<(), VoiceError> {
        let capture = self
            .capture
            .as_ref()
            .ok_or(VoiceError::NotInitialized)?;

        let handle = capture.start_recording()?;
        *self.recording.lock().await = Some(handle);
        *self.state.lock().await = VoiceState::Listening;
        self.vad.lock().await.reset();

        tracing::info!("voice: listening started");
        Ok(())
    }

    pub async fn stop_listening_and_transcribe(&self) -> Result<String, VoiceError> {
        *self.state.lock().await = VoiceState::Transcribing;

        let handle = self
            .recording
            .lock()
            .await
            .take()
            .ok_or(VoiceError::NotInitialized)?;

        let audio = handle.finish();

        if audio.is_empty() {
            *self.state.lock().await = VoiceState::Idle;
            return Ok(String::new());
        }

        let stt = self
            .stt
            .clone()
            .ok_or(VoiceError::NotInitialized)?;

        let text = tokio::task::spawn_blocking(move || stt.transcribe(&audio))
            .await
            .map_err(|e| VoiceError::Transcription(format!("task join error: {}", e)))?
            ?;

        *self.state.lock().await = VoiceState::Idle;
        tracing::info!("voice: transcribed '{}' ", text);
        Ok(text)
    }

    pub async fn speak(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        let tts = self.tts.as_ref().ok_or(VoiceError::Synthesis(
            "TTS not initialized".into(),
        ))?;

        *self.state.lock().await = VoiceState::Speaking;

        let tts = tts.clone();
        let text = text.to_string();
        let wav = tokio::task::spawn_blocking(move || tts.synthesize_to_wav(&text))
            .await
            .map_err(|e| VoiceError::Synthesis(format!("task join error: {}", e)))?
            ?;

        *self.state.lock().await = VoiceState::Idle;
        Ok(wav)
    }

    pub async fn check_vad(&self) -> VadEvent {
        let recording = self.recording.lock().await;
        if let Some(handle) = recording.as_ref() {
            let samples = handle.get_samples();
            if samples.len() >= 1600 {
                let tail = &samples[samples.len().saturating_sub(1600)..];
                return self.vad.lock().await.process(tail);
            }
        }
        VadEvent::Silence
    }
}
