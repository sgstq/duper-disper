use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::context::CapturedContext;

/// Configuration for the LLM refinement step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementConfig {
    /// API endpoint (OpenAI-compatible). Supports local (Ollama, LM Studio) or cloud.
    pub api_url: String,
    /// API key (empty for local models).
    pub api_key: String,
    /// Model name (e.g., "gpt-4o-mini", "llama3", "claude-sonnet-4-20250514").
    pub model: String,
    /// System prompt template. Use {context} and {transcript} placeholders.
    pub system_prompt: String,
    /// Whether to include screenshot in the request (vision model required).
    pub use_screenshot: bool,
    /// Max tokens for the response.
    pub max_tokens: u32,
}

impl Default for RefinementConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:11434/v1/chat/completions".to_string(),
            api_key: String::new(),
            model: "llama3".to_string(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            use_screenshot: false,
            max_tokens: 2048,
        }
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a voice transcription refinement assistant. Your job is to take raw speech-to-text output and produce clean, well-formatted text ready for insertion.

Rules:
- Fix grammar, punctuation, and capitalization
- Remove filler words (um, uh, like, you know) unless they add meaning
- Maintain the speaker's intent and tone
- Format appropriately for the context (email, code comment, chat message, etc.)
- Do NOT add information that wasn't in the original speech
- Output ONLY the refined text, no explanations

Context about where the text will be inserted:
Application: {app_name}
Window title: {window_title}
Surrounding text: {surrounding_text}

Raw transcript:
{transcript}"#;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

pub struct Refiner {
    config: RefinementConfig,
    client: reqwest::Client,
}

impl Refiner {
    pub fn new(config: RefinementConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }

    /// Refine a raw transcript using the configured LLM.
    pub async fn refine(
        &self,
        transcript: &str,
        context: &CapturedContext,
    ) -> Result<String> {
        info!("Refining transcript ({} chars) with context", transcript.len());

        let system_prompt = self
            .config
            .system_prompt
            .replace("{app_name}", &context.app_name)
            .replace("{window_title}", &context.window_title)
            .replace("{surrounding_text}", &context.surrounding_text)
            .replace("{transcript}", transcript);

        let mut messages = vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::Value::String(system_prompt),
        }];

        // If we have a screenshot and config says to use it, add as vision content
        if self.config.use_screenshot {
            if let Some(ref screenshot_b64) = context.screenshot_base64 {
                messages = vec![ChatMessage {
                    role: "user".to_string(),
                    content: serde_json::json!([
                        {
                            "type": "text",
                            "text": self.config.system_prompt
                                .replace("{app_name}", &context.app_name)
                                .replace("{window_title}", &context.window_title)
                                .replace("{surrounding_text}", &context.surrounding_text)
                                .replace("{transcript}", transcript)
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{}", screenshot_b64)
                            }
                        }
                    ]),
                }];
            }
        }

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: 0.3,
        };

        debug!("Sending refinement request to {}", self.config.api_url);

        let mut req = self.client.post(&self.config.api_url).json(&request);

        if !self.config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = req
            .send()
            .await
            .context("Failed to send refinement request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Refinement API error ({}): {}", status, body);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse refinement response")?;

        let refined = chat_response
            .choices
            .first()
            .map(|c| c.message.content.trim().to_string())
            .unwrap_or_else(|| transcript.to_string());

        info!("Refinement complete: {} chars", refined.len());
        Ok(refined)
    }
}
