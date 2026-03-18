use anyhow::{Context, Result};
use std::path::Path;
use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::TranscriptionResult;

pub struct LocalTranscriber {
    ctx: WhisperContext,
    language: Option<String>,
}

impl LocalTranscriber {
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

    pub fn transcribe(&self, samples: &[f32]) -> Result<TranscriptionResult> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

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

        debug!("Running local whisper inference on {} samples", samples.len());

        state
            .full(params, samples)
            .map_err(|e| anyhow::anyhow!("Whisper inference failed: {}", e))?;

        let num_segments = state
            .full_n_segments()
            .map_err(|e| anyhow::anyhow!("Failed to get segments: {}", e))?;
        let mut text = String::new();

        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                text.push_str(&segment);
            }
        }

        let text = text.trim().to_string();
        info!("Local transcription complete: {} chars", text.len());

        Ok(TranscriptionResult { text })
    }
}

fn num_cpus() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .min(8)
}
