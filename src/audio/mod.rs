use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

/// Raw audio samples captured from the microphone (f32, mono, 16kHz).
pub struct AudioCapture {
    device: Device,
    config: StreamConfig,
    sample_format: SampleFormat,
}

/// Buffer that accumulates samples while recording.
#[derive(Clone)]
pub struct RecordingBuffer {
    samples: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
}

impl RecordingBuffer {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::with_capacity(16000 * 60))), // pre-alloc ~1 min
            is_recording: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self) {
        self.samples.lock().unwrap().clear();
        self.is_recording.store(true, Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.is_recording.store(false, Ordering::SeqCst);
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn push_samples(&self, data: &[f32]) {
        if self.is_recording.load(Ordering::SeqCst) {
            self.samples.lock().unwrap().extend_from_slice(data);
        }
    }

    /// Take all samples out of the buffer, leaving it empty.
    pub fn take_samples(&self) -> Vec<f32> {
        std::mem::take(&mut *self.samples.lock().unwrap())
    }

    pub fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No input audio device found")?;

        info!("Using input device: {:?}", device.name()?);

        let supported = device.default_input_config()?;
        let sample_format = supported.sample_format();

        // We want mono 16kHz for Whisper, but capture at device native rate
        // and resample later.
        let config: StreamConfig = supported.into();
        debug!(
            "Audio config: {} channels, {} Hz, format: {:?}",
            config.channels, config.sample_rate.0, sample_format
        );

        Ok(Self {
            device,
            config,
            sample_format,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn channels(&self) -> u16 {
        self.config.channels
    }

    /// Start a background capture stream. Samples are pushed into `buffer`.
    /// Returns a handle that stops the stream when dropped.
    pub fn start_stream(&self, buffer: RecordingBuffer) -> Result<CaptureStream> {
        let channels = self.config.channels as usize;
        let err_fn = |err| error!("Audio stream error: {}", err);

        let stream = match self.sample_format {
            SampleFormat::F32 => {
                let buf = buffer.clone();
                self.device.build_input_stream(
                    &self.config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        // Mix to mono
                        let mono: Vec<f32> = if channels == 1 {
                            data.to_vec()
                        } else {
                            data.chunks(channels)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect()
                        };
                        buf.push_samples(&mono);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let buf = buffer.clone();
                self.device.build_input_stream(
                    &self.config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = if channels == 1 {
                            data.iter().map(|&s| s as f32 / 32768.0).collect()
                        } else {
                            data.chunks(channels)
                                .map(|frame| {
                                    frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                        / channels as f32
                                })
                                .collect()
                        };
                        buf.push_samples(&mono);
                    },
                    err_fn,
                    None,
                )?
            }
            _ => anyhow::bail!("Unsupported sample format: {:?}", self.sample_format),
        };

        stream.play()?;
        info!("Audio capture stream started");

        Ok(CaptureStream { _stream: stream })
    }
}

pub struct CaptureStream {
    _stream: cpal::Stream,
}

/// Resample audio from `from_rate` to `to_rate` using linear interpolation.
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < samples.len() {
            samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac
        } else {
            samples[idx.min(samples.len() - 1)] as f64
        };

        output.push(sample as f32);
    }

    output
}

/// Compute the root-mean-square (RMS) energy of audio samples.
/// Returns a value in [0.0, 1.0] for normalized f32 audio.
pub fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// Save samples to a WAV file (for debugging).
pub fn save_wav(samples: &[f32], sample_rate: u32, path: &str) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RecordingBuffer tests ----

    #[test]
    fn buffer_new_is_empty() {
        let buf = RecordingBuffer::new();
        assert_eq!(buf.sample_count(), 0);
        assert!(!buf.is_recording());
    }

    #[test]
    fn buffer_start_sets_recording_flag() {
        let buf = RecordingBuffer::new();
        buf.start();
        assert!(buf.is_recording());
    }

    #[test]
    fn buffer_stop_clears_recording_flag() {
        let buf = RecordingBuffer::new();
        buf.start();
        buf.stop();
        assert!(!buf.is_recording());
    }

    #[test]
    fn buffer_accumulates_samples_while_recording() {
        let buf = RecordingBuffer::new();
        buf.start();
        buf.push_samples(&[0.1, 0.2, 0.3]);
        buf.push_samples(&[0.4, 0.5]);
        assert_eq!(buf.sample_count(), 5);
    }

    #[test]
    fn buffer_ignores_samples_when_not_recording() {
        let buf = RecordingBuffer::new();
        // Not started yet
        buf.push_samples(&[0.1, 0.2, 0.3]);
        assert_eq!(buf.sample_count(), 0);
    }

    #[test]
    fn buffer_ignores_samples_after_stop() {
        let buf = RecordingBuffer::new();
        buf.start();
        buf.push_samples(&[0.1, 0.2]);
        buf.stop();
        buf.push_samples(&[0.3, 0.4]);
        assert_eq!(buf.sample_count(), 2);
    }

    #[test]
    fn buffer_take_samples_returns_all_and_clears() {
        let buf = RecordingBuffer::new();
        buf.start();
        buf.push_samples(&[0.1, 0.2, 0.3]);
        let samples = buf.take_samples();
        assert_eq!(samples, vec![0.1, 0.2, 0.3]);
        assert_eq!(buf.sample_count(), 0);
    }

    #[test]
    fn buffer_start_clears_previous_samples() {
        let buf = RecordingBuffer::new();
        buf.start();
        buf.push_samples(&[0.1, 0.2]);
        buf.stop();
        buf.start(); // Should clear
        assert_eq!(buf.sample_count(), 0);
    }

    #[test]
    fn buffer_clone_shares_state() {
        let buf = RecordingBuffer::new();
        let buf2 = buf.clone();
        buf.start();
        buf2.push_samples(&[1.0, 2.0]);
        assert_eq!(buf.sample_count(), 2);
    }

    // ---- resample tests ----

    #[test]
    fn resample_same_rate_returns_copy() {
        let samples = vec![0.1, 0.2, 0.3, 0.4];
        let result = resample(&samples, 16000, 16000);
        assert_eq!(result, samples);
    }

    #[test]
    fn resample_downsample_halves_length() {
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32) / 1000.0).collect();
        let result = resample(&samples, 48000, 16000);
        // 48000->16000 is 3:1, so ~333 samples
        let expected_len = (1000.0 * 16000.0 / 48000.0) as usize;
        assert_eq!(result.len(), expected_len);
    }

    #[test]
    fn resample_upsample_increases_length() {
        let samples: Vec<f32> = (0..100).map(|i| (i as f32) / 100.0).collect();
        let result = resample(&samples, 16000, 48000);
        assert!(result.len() > samples.len());
        let expected_len = (100.0 * 48000.0 / 16000.0) as usize;
        assert_eq!(result.len(), expected_len);
    }

    #[test]
    fn resample_preserves_first_sample() {
        let samples = vec![0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
        let result = resample(&samples, 48000, 16000);
        assert!((result[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn resample_empty_input() {
        let result = resample(&[], 44100, 16000);
        assert!(result.is_empty());
    }

    #[test]
    fn resample_single_sample() {
        let result = resample(&[0.5], 44100, 16000);
        // Single sample should still produce at least something
        assert!(!result.is_empty());
    }

    // ---- rms_energy tests ----

    #[test]
    fn rms_energy_silence_is_zero() {
        assert_eq!(rms_energy(&[0.0; 1000]), 0.0);
    }

    #[test]
    fn rms_energy_empty_is_zero() {
        assert_eq!(rms_energy(&[]), 0.0);
    }

    #[test]
    fn rms_energy_full_scale_sine() {
        // RMS of a sine wave is 1/sqrt(2) ≈ 0.707
        let samples: Vec<f32> = (0..16000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin())
            .collect();
        let rms = rms_energy(&samples);
        assert!((rms - 0.707).abs() < 0.01, "RMS was {}", rms);
    }

    #[test]
    fn rms_energy_quiet_audio_below_threshold() {
        // Very quiet audio (amplitude 0.001) should have RMS well below 0.005
        let samples: Vec<f32> = vec![0.001; 1000];
        let rms = rms_energy(&samples);
        assert!(rms < 0.005, "RMS was {}", rms);
    }

    #[test]
    fn resample_interpolates_linearly() {
        // Simple case: 2:1 downsample
        let samples = vec![0.0, 1.0, 0.0, 1.0];
        let result = resample(&samples, 2, 1);
        // At ratio 2:1, output[0] = samples[0] = 0.0, output[1] = samples[2] = 0.0
        assert_eq!(result.len(), 2);
        assert!((result[0] - 0.0).abs() < 0.01);
    }
}
