use std::path::Path;
use std::sync::Arc;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::VoiceError;

pub struct WhisperStt {
    ctx: Arc<WhisperContext>,
}

impl WhisperStt {
    pub fn new(model_path: &Path) -> Result<Self, VoiceError> {
        if !model_path.exists() {
            return Err(VoiceError::ModelNotFound(
                model_path.display().to_string(),
            ));
        }

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or_default(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| VoiceError::Transcription(format!("failed to load model: {}", e)))?;

        Ok(Self {
            ctx: Arc::new(ctx),
        })
    }

    pub fn transcribe(&self, audio: &[f32]) -> Result<String, VoiceError> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| VoiceError::Transcription(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(true);
        params.set_no_context(true);

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4) as i32)
            .unwrap_or(2);
        params.set_n_threads(n_threads);

        state
            .full(params, audio)
            .map_err(|e| VoiceError::Transcription(e.to_string()))?;

        let n_segments = state.full_n_segments();

        let mut text = String::new();
        for i in 0..n_segments {
            if let Some(segment) = state.get_segment(i)
                && let Ok(s) = segment.to_str_lossy()
            {
                text.push_str(&s);
            }
        }

        Ok(text.trim().to_string())
    }
}

unsafe impl Send for WhisperStt {}
unsafe impl Sync for WhisperStt {}
