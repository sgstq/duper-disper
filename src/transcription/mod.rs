use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Transcriber {
    ctx: WhisperContext,
    language: Option<String>,
}

impl Transcriber {
    /// Create a new transcriber from a ggml whisper model file.
    pub fn new(model_path: &Path, language: Option<String>) -> Result<Self> {
        info!("Loading Whisper model from {:?}", model_path);
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().context("Invalid model path")?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to load Whisper model: {}", e))?;

        info!("Whisper model loaded successfully");
        Ok(Self { ctx, language })
    }

    /// Transcribe audio samples (mono f32, 16kHz).
    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult> {
        let mut state = self.ctx.create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Configure
        params.set_n_threads(num_cpus());
        params.set_translate(false);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(false);
        params.set_suppress_blank(true);

        if let Some(ref lang) = self.language {
            params.set_language(Some(lang));
        }

        debug!("Running whisper inference on {} samples", samples.len());

        state
            .full(params, samples)
            .map_err(|e| anyhow::anyhow!("Whisper inference failed: {}", e))?;

        let num_segments = state.full_n_segments()
            .map_err(|e| anyhow::anyhow!("Failed to get segments: {}", e))?;
        let mut text = String::new();

        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                text.push_str(&segment);
            }
        }

        let text = text.trim().to_string();
        info!("Transcription complete: {} chars", text.len());

        Ok(TranscriptionResult { text })
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
}

fn num_cpus() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .min(8) // cap at 8 threads for whisper
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

    // We'll download synchronously here since this is a one-time setup
    let response = reqwest::blocking::get(&url)
        .context("Failed to download model")?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download model: HTTP {}", response.status());
    }

    let bytes = response.bytes()?;
    std::fs::write(&model_file, &bytes)?;
    info!("Model saved to {:?} ({} MB)", model_file, bytes.len() / 1_000_000);

    Ok(model_file)
}
