use crate::insertion::InsertionMethod;
use crate::refinement::RefinementConfig;
use crate::transcription::{CloudSttConfig, SttBackend};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Global hotkey for push-to-talk (e.g., "CapsLock", "Ctrl+Shift+Space").
    pub hotkey: String,

    /// STT backend: "local", "openai", "deepgram", "groq".
    pub stt_backend: SttBackend,

    /// Whisper model name for local backend (e.g., "base.en", "small", "medium", "large-v3").
    pub whisper_model: String,

    /// Language code (e.g., "en", "auto" for auto-detect). Used by both local and cloud.
    pub language: String,

    /// Cloud STT configuration (used when stt_backend is not "local").
    pub cloud_stt: CloudSttConfig,

    /// How to insert text into the active app.
    pub insertion_method: String,

    /// Whether to refine transcripts with an LLM.
    pub enable_refinement: bool,

    /// LLM refinement configuration.
    pub refinement: RefinementConfig,

    /// Whether to capture screenshots for context.
    pub capture_screenshots: bool,

    /// Audio input device name (empty = default).
    pub audio_device: String,

    /// Play sound feedback on start/stop recording.
    pub sound_feedback: bool,

    /// Show overlay notification during recording.
    pub show_overlay: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "CapsLock".to_string(),
            stt_backend: SttBackend::Local,
            whisper_model: "base.en".to_string(),
            language: "en".to_string(),
            cloud_stt: CloudSttConfig::default(),
            insertion_method: "clipboard".to_string(),
            enable_refinement: true,
            refinement: RefinementConfig::default(),
            capture_screenshots: false,
            audio_device: String::new(),
            sound_feedback: true,
            show_overlay: true,
        }
    }
}

impl AppConfig {
    pub fn insertion_method(&self) -> InsertionMethod {
        match self.insertion_method.to_lowercase().as_str() {
            "typing" | "simulate" => InsertionMethod::SimulateTyping,
            _ => InsertionMethod::Clipboard,
        }
    }

    pub fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Cannot determine config directory")?
            .join("duper-disper");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn models_dir() -> Result<PathBuf> {
        let dir = dirs::data_local_dir()
            .context("Cannot determine data directory")?
            .join("duper-disper")
            .join("models");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let config: Self = toml::from_str(&content)
                .context("Failed to parse config file")?;
            info!("Config loaded from {:?}", path);
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            info!("Created default config at {:?}", path);
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}
