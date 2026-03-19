pub mod cloud;
#[cfg(feature = "local-stt")]
pub mod local;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
}

/// Which STT backend to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SttBackend {
    /// Local whisper.cpp via whisper-rs.
    Local,
    /// OpenAI Whisper API (or any compatible endpoint).
    OpenAI,
    /// Deepgram cloud API.
    Deepgram,
    /// Groq (fast cloud whisper).
    Groq,
}

impl Default for SttBackend {
    fn default() -> Self {
        Self::Local
    }
}

/// Configuration for cloud STT providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSttConfig {
    /// API endpoint URL. Leave empty to use provider defaults.
    pub api_url: String,
    /// API key for the cloud provider.
    pub api_key: String,
    /// Model name (provider-specific, e.g. "whisper-1", "whisper-large-v3", "nova-2").
    pub model: String,
}

impl Default for CloudSttConfig {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

/// Unified transcriber that delegates to the configured backend.
pub enum Transcriber {
    #[cfg(feature = "local-stt")]
    Local(local::LocalTranscriber),
    Cloud(cloud::CloudTranscriber),
}

impl Transcriber {
    /// Create a local (whisper.cpp) transcriber.
    #[cfg(feature = "local-stt")]
    pub fn new_local(model_path: &Path, language: Option<String>) -> Result<Self> {
        Ok(Self::Local(local::LocalTranscriber::new(model_path, language)?))
    }

    /// Create a cloud transcriber.
    pub fn new_cloud(backend: SttBackend, config: CloudSttConfig, language: String) -> Result<Self> {
        Ok(Self::Cloud(cloud::CloudTranscriber::new(backend, config, language)?))
    }

    /// Transcribe audio samples (mono f32, 16kHz).
    /// For cloud backends, this encodes to WAV and uploads.
    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult> {
        match self {
            #[cfg(feature = "local-stt")]
            Self::Local(t) => t.transcribe(samples),
            Self::Cloud(t) => t.transcribe(samples),
        }
    }
}

/// Known Whisper hallucination phrases produced on silent or near-silent audio.
const WHISPER_HALLUCINATIONS: &[&str] = &[
    "thank you",
    "thanks for watching",
    "thank you for watching",
    "thanks for listening",
    "thank you for listening",
    "subtitles by",
    "subtitled by",
    "you",
    "bye",
    "the end",
];

/// Returns true if the transcription looks like a Whisper hallucination
/// (a short, known phrase that Whisper produces on silence).
pub fn is_hallucination(text: &str) -> bool {
    let normalized = text
        .trim()
        .trim_end_matches(|c: char| c == '.' || c == '!' || c == ',' || c == '?')
        .to_lowercase();

    WHISPER_HALLUCINATIONS
        .iter()
        .any(|&h| normalized == h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stt_backend_default_is_local() {
        assert_eq!(SttBackend::default(), SttBackend::Local);
    }

    #[test]
    fn cloud_stt_config_defaults_are_empty() {
        let config = CloudSttConfig::default();
        assert!(config.api_url.is_empty());
        assert!(config.api_key.is_empty());
        assert!(config.model.is_empty());
    }

    #[test]
    fn transcription_result_holds_text() {
        let result = TranscriptionResult {
            text: "Hello world".to_string(),
        };
        assert_eq!(result.text, "Hello world");
    }

    #[test]
    fn hallucination_detects_thank_you() {
        assert!(is_hallucination("Thank you."));
        assert!(is_hallucination("thank you"));
        assert!(is_hallucination(" Thank you. "));
        assert!(is_hallucination("Thank you!"));
        assert!(is_hallucination("Thank you,"));
    }

    #[test]
    fn hallucination_detects_other_phrases() {
        assert!(is_hallucination("Thanks for watching."));
        assert!(is_hallucination("Subtitles by"));
        assert!(is_hallucination("Bye."));
        assert!(is_hallucination("You"));
        assert!(is_hallucination("The end."));
    }

    #[test]
    fn hallucination_rejects_real_speech() {
        assert!(!is_hallucination("Thank you for the update on the project"));
        assert!(!is_hallucination("Please send me the report"));
        assert!(!is_hallucination("Hello world"));
        assert!(!is_hallucination("I need to thank you for your help"));
    }

    #[test]
    fn stt_backend_roundtrip_serialization() {
        for backend in [SttBackend::Local, SttBackend::OpenAI, SttBackend::Deepgram, SttBackend::Groq] {
            let json = serde_json::to_string(&backend).unwrap();
            let deserialized: SttBackend = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, backend);
        }
    }
}

/// Find or download a whisper model. Returns path to the model file.
pub fn ensure_model(model_name: &str, models_dir: &Path) -> Result<PathBuf> {
    let model_file = models_dir.join(format!("ggml-{}.bin", model_name));

    if model_file.exists() {
        info!("Model found at {:?}", model_file);
        return Ok(model_file);
    }

    std::fs::create_dir_all(models_dir)?;

    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        model_name
    );
    info!("Downloading model from {}", url);

    let response = reqwest::blocking::get(&url)
        .map_err(|e| anyhow::anyhow!("Failed to download model: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download model: HTTP {}", response.status());
    }

    let bytes = response.bytes()?;
    std::fs::write(&model_file, &bytes)?;
    info!("Model saved to {:?} ({} MB)", model_file, bytes.len() / 1_000_000);

    Ok(model_file)
}
