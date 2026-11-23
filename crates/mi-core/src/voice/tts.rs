use super::VoiceError;

#[cfg(windows)]
mod windows_tts {
    use super::VoiceError;
    use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
    use windows::Storage::Streams::DataReader;

    pub struct WindowsTts {
        synth: SpeechSynthesizer,
    }

    impl WindowsTts {
        pub fn new() -> Result<Self, VoiceError> {
            let synth = SpeechSynthesizer::new()
                .map_err(|e| VoiceError::Synthesis(format!("failed to create synthesizer: {}", e)))?;

            if let Some(voice) = pick_female_voice() {
                if let Ok(name) = voice.DisplayName() {
                    tracing::info!("voice: using TTS voice '{}'", name);
                }
                let _ = synth.SetVoice(&voice);
            }

            Ok(Self { synth })
        }

        pub fn synthesize_to_wav(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
            if text.is_empty() {
                return Ok(Vec::new());
            }

            let text_hstring: windows::core::HSTRING = text.into();
            let async_op = self
                .synth
                .SynthesizeTextToStreamAsync(&text_hstring)
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            let stream = futures::executor::block_on(async { async_op.await })
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            let size = stream
                .Size()
                .map_err(|e| VoiceError::Synthesis(e.to_string()))? as u32;

            if size == 0 {
                return Ok(Vec::new());
            }

            let input_stream = stream
                .GetInputStreamAt(0)
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            let reader = DataReader::CreateDataReader(&input_stream)
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            let load_op = reader
                .LoadAsync(size)
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            futures::executor::block_on(async { load_op.await })
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            let mut buffer = vec![0u8; size as usize];
            reader
                .ReadBytes(&mut buffer)
                .map_err(|e| VoiceError::Synthesis(e.to_string()))?;

            Ok(buffer)
        }
    }

    /// Pick a female voice, preferring well-known pleasant en-US voices
    /// (Zira / Aria / Jenny), else any female voice installed.
    fn pick_female_voice() -> Option<VoiceInformation> {
        let voices = SpeechSynthesizer::AllVoices().ok()?;
        let size = voices.Size().ok()?;

        let mut first_female: Option<VoiceInformation> = None;
        for i in 0..size {
            let Ok(voice) = voices.GetAt(i) else {
                continue;
            };
            let is_female = voice
                .Gender()
                .map(|g| g == VoiceGender::Female)
                .unwrap_or(false);
            if !is_female {
                continue;
            }

            if let Ok(name) = voice.DisplayName() {
                let name = name.to_string_lossy();
                if name.contains("Zira")
                    || name.contains("Aria")
                    || name.contains("Jenny")
                    || name.contains("Michelle")
                {
                    return Some(voice);
                }
            }
            if first_female.is_none() {
                first_female = Some(voice);
            }
        }
        first_female
    }
}

#[cfg(windows)]
pub use windows_tts::WindowsTts;

#[cfg(not(windows))]
pub struct WindowsTts;

#[cfg(not(windows))]
impl WindowsTts {
    pub fn new() -> Result<Self, VoiceError> {
        Err(VoiceError::Synthesis(
            "TTS not available on this platform yet".into(),
        ))
    }

    pub fn synthesize_to_wav(&self, _text: &str) -> Result<Vec<u8>, VoiceError> {
        Err(VoiceError::Synthesis(
            "TTS not available on this platform yet".into(),
        ))
    }
}
