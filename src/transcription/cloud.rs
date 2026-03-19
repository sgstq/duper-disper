use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, info};

use super::{CloudSttConfig, SttBackend, TranscriptionResult};

/// Cloud-based transcriber supporting OpenAI Whisper API, Deepgram, and Groq.
#[derive(Debug)]
pub struct CloudTranscriber {
    backend: SttBackend,
    config: CloudSttConfig,
    language: String,
    client: reqwest::blocking::Client,
}

impl CloudTranscriber {
    pub fn new(backend: SttBackend, config: CloudSttConfig, language: String) -> Result<Self> {
        if config.api_key.is_empty() {
            anyhow::bail!(
                "API key required for {:?} cloud STT. Set it in config.toml under [cloud_stt].",
                backend
            );
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        info!("Cloud STT initialized: backend={:?}, model={}", backend, config.model);
        Ok(Self {
            backend,
            config,
            language,
            client,
        })
    }

    /// Transcribe audio samples by encoding to WAV and uploading to the cloud API.
    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult> {
        let wav_data = encode_wav(samples, 16000)?;
        debug!(
            "Encoded {} samples to {} bytes WAV for cloud upload",
            samples.len(),
            wav_data.len()
        );

        match self.backend {
            SttBackend::OpenAI => self.transcribe_openai(&wav_data),
            SttBackend::Deepgram => self.transcribe_deepgram(&wav_data),
            SttBackend::Groq => self.transcribe_groq(&wav_data),
            SttBackend::Local => unreachable!("Local backend should not reach cloud transcriber"),
        }
    }

    /// OpenAI Whisper API (POST /v1/audio/transcriptions, multipart form).
    /// Also works with any OpenAI-compatible endpoint.
    fn transcribe_openai(&self, wav_data: &[u8]) -> Result<TranscriptionResult> {
        let url = if self.config.api_url.is_empty() {
            "https://api.openai.com/v1/audio/transcriptions".to_string()
        } else {
            self.config.api_url.clone()
        };

        let model = if self.config.model.is_empty() {
            "whisper-1"
        } else {
            &self.config.model
        };

        info!("Sending audio to OpenAI Whisper API (model={})", model);

        let file_part = reqwest::blocking::multipart::Part::bytes(wav_data.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = reqwest::blocking::multipart::Form::new()
            .part("file", file_part)
            .text("model", model.to_string())
            .text("response_format", "json");

        if !self.language.is_empty() && self.language != "auto" {
            form = form.text("language", self.language.clone());
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .multipart(form)
            .send()
            .context("Failed to send request to OpenAI Whisper API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, body);
        }

        let result: OpenAiTranscriptionResponse = response
            .json()
            .context("Failed to parse OpenAI response")?;

        info!("OpenAI transcription complete: {} chars", result.text.len());
        Ok(TranscriptionResult {
            text: result.text.trim().to_string(),
        })
    }

    /// Deepgram API (POST /v1/listen, raw audio body).
    fn transcribe_deepgram(&self, wav_data: &[u8]) -> Result<TranscriptionResult> {
        let base_url = if self.config.api_url.is_empty() {
            "https://api.deepgram.com/v1/listen"
        } else {
            &self.config.api_url
        };

        let model = if self.config.model.is_empty() {
            "nova-2"
        } else {
            &self.config.model
        };

        let mut url = format!("{}?model={}&smart_format=true", base_url, model);
        if !self.language.is_empty() && self.language != "auto" {
            url.push_str(&format!("&language={}", self.language));
        }

        info!("Sending audio to Deepgram API (model={})", model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Token {}", self.config.api_key))
            .header("Content-Type", "audio/wav")
            .body(wav_data.to_vec())
            .send()
            .context("Failed to send request to Deepgram API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("Deepgram API error ({}): {}", status, body);
        }

        let result: DeepgramResponse = response
            .json()
            .context("Failed to parse Deepgram response")?;

        let text = result
            .results
            .channels
            .first()
            .and_then(|ch| ch.alternatives.first())
            .map(|alt| alt.transcript.clone())
            .unwrap_or_default();

        info!("Deepgram transcription complete: {} chars", text.len());
        Ok(TranscriptionResult {
            text: text.trim().to_string(),
        })
    }

    /// Groq API (uses OpenAI-compatible endpoint with Groq's whisper models).
    fn transcribe_groq(&self, wav_data: &[u8]) -> Result<TranscriptionResult> {
        let url = if self.config.api_url.is_empty() {
            "https://api.groq.com/openai/v1/audio/transcriptions".to_string()
        } else {
            self.config.api_url.clone()
        };

        let model = if self.config.model.is_empty() {
            "whisper-large-v3-turbo"
        } else {
            &self.config.model
        };

        info!("Sending audio to Groq API (model={})", model);

        let file_part = reqwest::blocking::multipart::Part::bytes(wav_data.to_vec())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = reqwest::blocking::multipart::Form::new()
            .part("file", file_part)
            .text("model", model.to_string())
            .text("response_format", "json");

        if !self.language.is_empty() && self.language != "auto" {
            form = form.text("language", self.language.clone());
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .multipart(form)
            .send()
            .context("Failed to send request to Groq API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("Groq API error ({}): {}", status, body);
        }

        // Groq uses OpenAI-compatible response format
        let result: OpenAiTranscriptionResponse = response
            .json()
            .context("Failed to parse Groq response")?;

        info!("Groq transcription complete: {} chars", result.text.len());
        Ok(TranscriptionResult {
            text: result.text.trim().to_string(),
        })
    }
}

// --- Response types ---

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

#[derive(Debug, Deserialize)]
struct DeepgramResponse {
    results: DeepgramResults,
}

#[derive(Debug, Deserialize)]
struct DeepgramResults {
    channels: Vec<DeepgramChannel>,
}

#[derive(Debug, Deserialize)]
struct DeepgramChannel {
    alternatives: Vec<DeepgramAlternative>,
}

#[derive(Debug, Deserialize)]
struct DeepgramAlternative {
    transcript: String,
}

// --- WAV encoding ---

/// Encode f32 mono samples at the given sample rate into a WAV byte buffer.
fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
    for &s in samples {
        let sample_i16 = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(sample_i16)?;
    }
    writer.finalize()?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_wav_produces_valid_wav() {
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.001).sin()).collect();
        let wav_data = encode_wav(&samples, 16000).unwrap();

        // WAV files start with "RIFF"
        assert_eq!(&wav_data[..4], b"RIFF");
        // Should contain "WAVE" marker
        assert_eq!(&wav_data[8..12], b"WAVE");
        // Should have reasonable size (header + 16-bit samples)
        assert!(wav_data.len() > 44); // WAV header is 44 bytes
    }

    #[test]
    fn encode_wav_empty_samples() {
        let wav_data = encode_wav(&[], 16000).unwrap();
        assert_eq!(&wav_data[..4], b"RIFF");
        // Only header, no sample data beyond standard WAV structure
    }

    #[test]
    fn encode_wav_clamps_extreme_values() {
        // Values beyond [-1.0, 1.0] should be clamped
        let samples = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
        let wav_data = encode_wav(&samples, 16000).unwrap();
        assert!(wav_data.len() > 44);

        // Decode back to verify clamping
        let cursor = std::io::Cursor::new(wav_data);
        let mut reader = hound::WavReader::new(cursor).unwrap();
        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded.len(), 5);
        assert_eq!(decoded[0], -32768); // clamped from -2.0
        assert_eq!(decoded[1], -32767); // -1.0 * 32767
        assert_eq!(decoded[2], 0);      // 0.0
        assert_eq!(decoded[3], 32767);  // 1.0 * 32767 (clamped)
        assert_eq!(decoded[4], 32767);  // clamped from 2.0
    }

    #[test]
    fn encode_wav_preserves_sample_count() {
        let samples: Vec<f32> = vec![0.0; 500];
        let wav_data = encode_wav(&samples, 44100).unwrap();
        let cursor = std::io::Cursor::new(wav_data);
        let reader = hound::WavReader::new(cursor).unwrap();
        assert_eq!(reader.len(), 500);
    }

    #[test]
    fn encode_wav_preserves_sample_rate() {
        let wav_data = encode_wav(&[0.0], 44100).unwrap();
        let cursor = std::io::Cursor::new(wav_data);
        let reader = hound::WavReader::new(cursor).unwrap();
        assert_eq!(reader.spec().sample_rate, 44100);
    }

    #[test]
    fn encode_wav_is_mono_16bit() {
        let wav_data = encode_wav(&[0.5], 16000).unwrap();
        let cursor = std::io::Cursor::new(wav_data);
        let reader = hound::WavReader::new(cursor).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    }

    #[test]
    fn cloud_transcriber_requires_api_key() {
        let result = CloudTranscriber::new(
            SttBackend::OpenAI,
            CloudSttConfig::default(), // empty api_key
            "en".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key required"));
    }
}
