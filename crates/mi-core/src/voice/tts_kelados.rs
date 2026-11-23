use super::VoiceError;

/// Text-to-speech via a local Kelados server (`npx kelados serve`), which
/// exposes an OpenAI-compatible `POST /v1/audio/speech` endpoint backed by
/// Kokoro voices. Audio and model stay on the local machine.
pub struct KeladosTts {
    base_url: String,
    voice: String,
    client: reqwest::Client,
}

impl KeladosTts {
    pub fn new(base_url: String, voice: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            voice,
            client: reqwest::Client::new(),
        }
    }

    /// Returns true if the local server answers `GET /health`.
    pub async fn is_up(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Synthesize `text` to WAV bytes (24 kHz 16-bit PCM WAV).
    pub async fn synthesize_to_wav(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/v1/audio/speech", self.base_url);
        let body = serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": self.voice,
            "response_format": "wav",
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                VoiceError::Synthesis(format!(
                    "kelados request failed (is `npx kelados serve` running at {}?): {}",
                    self.base_url, e
                ))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Synthesis(format!(
                "kelados returned {}: {}",
                status, detail
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Synthesis(e.to_string()))?;
        Ok(bytes.to_vec())
    }
}
