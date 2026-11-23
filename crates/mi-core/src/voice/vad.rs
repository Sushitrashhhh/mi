use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VadEvent {
    Speech,
    Silence,
    SilenceTimeout,
}

pub struct VoiceActivityDetector {
    threshold: f32,
    silence_timeout: Duration,
    last_speech: Option<Instant>,
    was_speaking: bool,
}

impl VoiceActivityDetector {
    pub fn new(threshold: f32, silence_timeout_s: f32) -> Self {
        Self {
            threshold,
            silence_timeout: Duration::from_secs_f32(silence_timeout_s),
            last_speech: None,
            was_speaking: false,
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> VadEvent {
        let rms = rms_energy(samples);
        let is_speech = rms > self.threshold;

        if is_speech {
            self.last_speech = Some(Instant::now());
            self.was_speaking = true;
            return VadEvent::Speech;
        }

        if self.was_speaking
            && let Some(last) = self.last_speech
            && last.elapsed() >= self.silence_timeout
        {
            self.was_speaking = false;
            self.last_speech = None;
            return VadEvent::SilenceTimeout;
        }

        VadEvent::Silence
    }

    pub fn reset(&mut self) {
        self.last_speech = None;
        self.was_speaking = false;
    }
}

fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_energy_silence() {
        let silence = vec![0.0; 1000];
        assert_eq!(rms_energy(&silence), 0.0);
    }

    #[test]
    fn test_rms_energy_loud() {
        let loud = vec![0.5; 1000];
        assert!((rms_energy(&loud) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_vad_detects_speech() {
        let mut vad = VoiceActivityDetector::new(0.01, 2.0);
        let loud = vec![0.1; 160];
        assert_eq!(vad.process(&loud), VadEvent::Speech);
    }

    #[test]
    fn test_vad_detects_silence() {
        let mut vad = VoiceActivityDetector::new(0.01, 2.0);
        let quiet = vec![0.001; 160];
        assert_eq!(vad.process(&quiet), VadEvent::Silence);
    }
}
