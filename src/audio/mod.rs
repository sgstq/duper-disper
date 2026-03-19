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
