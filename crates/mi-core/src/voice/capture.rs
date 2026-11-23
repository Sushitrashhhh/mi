use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::VoiceError;

const TARGET_SAMPLE_RATE: u32 = 16000;

pub struct AudioCapture {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
}

pub struct RecordingHandle {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    source_sample_rate: u32,
}

impl AudioCapture {
    pub fn new() -> Result<Self, VoiceError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(VoiceError::NoInputDevice)?;

        let config = device
            .default_input_config()
            .map_err(|e| VoiceError::AudioStream(e.to_string()))?;

        Ok(Self { device, config })
    }

    pub fn start_recording(&self) -> Result<RecordingHandle, VoiceError> {
        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buffer.clone();
        let channels = self.config.channels() as usize;

        let err_fn = |e: cpal::Error| {
            tracing::error!("audio capture error: {}", e);
        };

        let stream = match self.config.sample_format() {
            cpal::SampleFormat::F32 => self.device.build_input_stream(
                self.config.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono = to_mono(data, channels);
                    if let Ok(mut buf) = buf_clone.lock() {
                        buf.extend_from_slice(&mono);
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let buf_clone = buffer.clone();
                self.device.build_input_stream(
                    self.config.config(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let floats: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        let mono = to_mono(&floats, channels);
                        if let Ok(mut buf) = buf_clone.lock() {
                            buf.extend_from_slice(&mono);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I32 => {
                let buf_clone = buffer.clone();
                self.device.build_input_stream(
                    self.config.config(),
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        let floats: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i32::MAX as f32).collect();
                        let mono = to_mono(&floats, channels);
                        if let Ok(mut buf) = buf_clone.lock() {
                            buf.extend_from_slice(&mono);
                        }
                    },
                    err_fn,
                    None,
                )
            }
            fmt => {
                return Err(VoiceError::AudioStream(format!(
                    "unsupported sample format: {:?}",
                    fmt
                )));
            }
        }
        .map_err(|e| VoiceError::AudioStream(e.to_string()))?;

        stream
            .play()
            .map_err(|e| VoiceError::AudioStream(e.to_string()))?;

        Ok(RecordingHandle {
            _stream: stream,
            buffer,
            source_sample_rate: self.config.sample_rate(),
        })
    }
}

impl RecordingHandle {
    pub fn get_samples(&self) -> Vec<f32> {
        self.buffer.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn finish(self) -> Vec<f32> {
        let raw = self
            .buffer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();

        if self.source_sample_rate == TARGET_SAMPLE_RATE {
            return raw;
        }

        resample(&raw, self.source_sample_rate, TARGET_SAMPLE_RATE)
    }
}

fn to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (input.len() as f64 * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let sample = if idx + 1 < input.len() {
            input[idx] * (1.0 - frac) + input[idx + 1] * frac
        } else {
            input[idx.min(input.len() - 1)]
        };
        output.push(sample);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_mono_stereo() {
        let stereo = vec![0.5, -0.5, 1.0, -1.0];
        let mono = to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 1e-6);
        assert!((mono[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_to_mono_already_mono() {
        let mono_in = vec![0.1, 0.2, 0.3];
        let mono_out = to_mono(&mono_in, 1);
        assert_eq!(mono_in, mono_out);
    }

    #[test]
    fn test_resample_identity() {
        let samples = vec![1.0, 2.0, 3.0];
        let out = resample(&samples, 16000, 16000);
        assert_eq!(samples, out);
    }

    #[test]
    fn test_resample_downsample() {
        let samples: Vec<f32> = (0..48000).map(|i| (i as f32).sin()).collect();
        let out = resample(&samples, 48000, 16000);
        assert!((out.len() as f32 - 16000.0).abs() < 2.0);
    }
}
