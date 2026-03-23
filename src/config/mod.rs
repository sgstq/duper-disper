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

    /// Developer mode: enable debug tracing, show logs, and expose troubleshooting tools.
    #[serde(default)]
    pub developer_mode: bool,

    /// Start application automatically on user login.
    #[serde(default)]
    pub auto_start: bool,
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
            developer_mode: false,
            auto_start: false,
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
        self.apply_auto_start();
        Ok(())
    }

    /// Sync the Windows auto-start registry key with the config value.
    #[cfg(windows)]
    fn apply_auto_start(&self) {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{
            RegDeleteValueW, RegSetValueExW, RegOpenKeyExW, RegCloseKey,
            HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
        };

        let subkey = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Run");
        let value_name = HSTRING::from("DuperDisper");

        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        let result = unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, 0, KEY_SET_VALUE, &mut hkey)
        };
        if result.is_err() {
            tracing::warn!("Failed to open auto-start registry key: {:?}", result);
            return;
        }

        if self.auto_start {
            match std::env::current_exe() {
                Ok(exe) => {
                    // Quote the path so spaces in "Program Files" etc. are handled.
                    // Use OsStr::encode_wide() to avoid lossy UTF-8 conversion that
                    // could corrupt paths containing non-Unicode WTF-16 sequences.
                    use std::os::windows::ffi::OsStrExt;
                    let quote: u16 = b'"' as u16;
                    let nul: u16 = 0;
                    let wide: Vec<u16> = std::iter::once(quote)
                        .chain(exe.as_os_str().encode_wide())
                        .chain(std::iter::once(quote))
                        .chain(std::iter::once(nul))
                        .collect();
                    let bytes: &[u8] = unsafe {
                        std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2)
                    };
                    let err = unsafe {
                        RegSetValueExW(hkey, &value_name, 0, REG_SZ, Some(bytes))
                    };
                    if err.is_err() {
                        tracing::warn!("Failed to set auto-start registry value: {:?}", err);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to determine exe path for auto-start: {}", e);
                }
            }
        } else {
            let err = unsafe { RegDeleteValueW(hkey, &value_name) };
            if err.is_err() {
                tracing::debug!("Failed to remove auto-start registry value: {:?}", err);
            }
        }

        unsafe { let _ = RegCloseKey(hkey); }
    }

    #[cfg(not(windows))]
    fn apply_auto_start(&self) {
        // No-op on non-Windows platforms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = AppConfig::default();
        assert_eq!(config.hotkey, "CapsLock");
        assert_eq!(config.stt_backend, SttBackend::Local);
        assert_eq!(config.whisper_model, "base.en");
        assert_eq!(config.language, "en");
        assert_eq!(config.insertion_method, "clipboard");
        assert!(config.enable_refinement);
        assert!(!config.capture_screenshots);
        assert!(config.audio_device.is_empty());
        assert!(config.sound_feedback);
        assert!(config.show_overlay);
        assert!(!config.developer_mode);
        assert!(!config.auto_start);
    }

    #[test]
    fn insertion_method_clipboard_default() {
        let config = AppConfig::default();
        assert_eq!(config.insertion_method(), InsertionMethod::Clipboard);
    }

    #[test]
    fn insertion_method_clipboard_explicit() {
        let mut config = AppConfig::default();
        config.insertion_method = "clipboard".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::Clipboard);
    }

    #[test]
    fn insertion_method_typing() {
        let mut config = AppConfig::default();
        config.insertion_method = "typing".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::SimulateTyping);
    }

    #[test]
    fn insertion_method_simulate() {
        let mut config = AppConfig::default();
        config.insertion_method = "simulate".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::SimulateTyping);
    }

    #[test]
    fn insertion_method_case_insensitive() {
        let mut config = AppConfig::default();
        config.insertion_method = "Typing".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::SimulateTyping);

        config.insertion_method = "CLIPBOARD".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::Clipboard);
    }

    #[test]
    fn insertion_method_unknown_defaults_to_clipboard() {
        let mut config = AppConfig::default();
        config.insertion_method = "unknown".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::Clipboard);

        config.insertion_method = "".to_string();
        assert_eq!(config.insertion_method(), InsertionMethod::Clipboard);
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.hotkey, config.hotkey);
        assert_eq!(deserialized.whisper_model, config.whisper_model);
        assert_eq!(deserialized.language, config.language);
        assert_eq!(deserialized.insertion_method, config.insertion_method);
        assert_eq!(deserialized.enable_refinement, config.enable_refinement);
        assert_eq!(deserialized.capture_screenshots, config.capture_screenshots);
        assert_eq!(deserialized.sound_feedback, config.sound_feedback);
        assert_eq!(deserialized.show_overlay, config.show_overlay);
        assert_eq!(deserialized.developer_mode, config.developer_mode);
        assert_eq!(deserialized.auto_start, config.auto_start);
    }

    #[test]
    fn config_deserializes_custom_values() {
        let toml_str = r#"
            hotkey = "F9"
            stt_backend = "openai"
            whisper_model = "large-v3"
            language = "auto"
            insertion_method = "typing"
            enable_refinement = false
            capture_screenshots = true
            audio_device = "Microphone (USB Audio)"
            sound_feedback = false
            show_overlay = false
            developer_mode = true

            [cloud_stt]
            api_url = "https://api.openai.com/v1/audio/transcriptions"
            api_key = "sk-test"
            model = "whisper-1"

            [refinement]
            api_url = "https://api.openai.com/v1/chat/completions"
            api_key = "sk-refine"
            model = "gpt-4o-mini"
            system_prompt = "Custom prompt {transcript}"
            use_screenshot = true
            max_tokens = 4096
        "#;

        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hotkey, "F9");
        assert_eq!(config.stt_backend, SttBackend::OpenAI);
        assert_eq!(config.whisper_model, "large-v3");
        assert_eq!(config.language, "auto");
        assert_eq!(config.insertion_method, "typing");
        assert!(!config.enable_refinement);
        assert!(config.capture_screenshots);
        assert_eq!(config.audio_device, "Microphone (USB Audio)");
        assert!(!config.sound_feedback);
        assert!(!config.show_overlay);
        assert!(config.developer_mode);
        assert_eq!(config.cloud_stt.api_key, "sk-test");
        assert_eq!(config.refinement.model, "gpt-4o-mini");
        assert!(config.refinement.use_screenshot);
        assert_eq!(config.refinement.max_tokens, 4096);
    }

    #[test]
    fn cloud_stt_config_defaults() {
        let config = CloudSttConfig::default();
        assert!(config.api_url.is_empty());
        assert!(config.api_key.is_empty());
        assert!(config.model.is_empty());
    }

    #[test]
    fn stt_backend_default_is_local() {
        assert_eq!(SttBackend::default(), SttBackend::Local);
    }

    #[test]
    fn stt_backend_serialization() {
        assert_eq!(serde_json::to_string(&SttBackend::Local).unwrap(), "\"local\"");
        assert_eq!(serde_json::to_string(&SttBackend::OpenAI).unwrap(), "\"openai\"");
        assert_eq!(serde_json::to_string(&SttBackend::Deepgram).unwrap(), "\"deepgram\"");
        assert_eq!(serde_json::to_string(&SttBackend::Groq).unwrap(), "\"groq\"");
    }

    #[test]
    fn stt_backend_deserialization() {
        assert_eq!(serde_json::from_str::<SttBackend>("\"local\"").unwrap(), SttBackend::Local);
        assert_eq!(serde_json::from_str::<SttBackend>("\"openai\"").unwrap(), SttBackend::OpenAI);
        assert_eq!(serde_json::from_str::<SttBackend>("\"deepgram\"").unwrap(), SttBackend::Deepgram);
        assert_eq!(serde_json::from_str::<SttBackend>("\"groq\"").unwrap(), SttBackend::Groq);
    }
}
