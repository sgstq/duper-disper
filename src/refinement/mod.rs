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

pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a text post-processor for a voice transcription tool. You receive raw speech-to-text output and return cleaned text.

CRITICAL RULES:
- Output ONLY the cleaned text. Nothing else. No preamble, no apology, no explanation.
- NEVER say "sorry", "I can't", "the transcription", "truncated", "incomplete", or comment on the input quality.
- NEVER complete, extend, or finish partial sentences. If the speaker said "Let's" and stopped, output "Let's" — do NOT guess what they meant to say.
- NEVER use the context (app name, window title) to invent or infer words the speaker did not say. Context is ONLY for formatting hints (e.g. capitalizing proper nouns).
- If the input is very short or a fragment, return it as-is with only minor cleanup. If truly unintelligible, return an empty string.
- Fix grammar, punctuation, and capitalization.
- Remove filler words (um, uh, like, you know) unless they add meaning.
- Maintain the speaker's intent and tone exactly.
- Do NOT add information that wasn't in the original speech.
- Do NOT wrap output in quotes or markdown.

Context (for formatting hints only):
Application: {app_name}
Window title: {window_title}

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

        // Guard: if the LLM returned a refusal or meta-commentary, fall back to raw transcript
        let refined = if is_llm_refusal(&refined) {
            info!("LLM returned refusal/meta-commentary, using raw transcript");
            transcript.to_string()
        } else {
            refined
        };

        info!("Refinement complete: {} chars", refined.len());
        Ok(refined)
    }
}

/// Detect if the LLM returned a refusal or meta-commentary instead of refined text.
fn is_llm_refusal(text: &str) -> bool {
    let lower = text.to_lowercase();
    let refusal_patterns = [
        "sorry",
        "i can't",
        "i cannot",
        "i'm unable",
        "the transcription",
        "appears to be",
        "seems to be",
        "truncated",
        "incomplete",
        "unintelligible",
        "not enough context",
        "please provide",
        "could you",
        "i apologize",
        "as an ai",
        "i'm an ai",
    ];
    refusal_patterns.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CapturedContext;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ---- is_llm_refusal tests ----

    #[test]
    fn refusal_detects_sorry() {
        assert!(is_llm_refusal("Sorry, I can't process that."));
    }

    #[test]
    fn refusal_detects_apology() {
        assert!(is_llm_refusal("I apologize, but the text is unclear."));
    }

    #[test]
    fn refusal_detects_as_an_ai() {
        assert!(is_llm_refusal("As an AI, I cannot determine the intent."));
    }

    #[test]
    fn refusal_detects_appears_to_be() {
        assert!(is_llm_refusal("This appears to be garbled audio."));
    }

    #[test]
    fn refusal_detects_truncated() {
        assert!(is_llm_refusal("The input seems truncated or incomplete."));
    }

    #[test]
    fn refusal_detects_case_insensitive() {
        assert!(is_llm_refusal("SORRY, I CANNOT HELP."));
        assert!(is_llm_refusal("The Transcription is unclear."));
    }

    #[test]
    fn refusal_accepts_clean_text() {
        assert!(!is_llm_refusal("Hello, how are you today?"));
        assert!(!is_llm_refusal("Meeting at 3pm tomorrow."));
        assert!(!is_llm_refusal("Please send the report by Friday."));
    }

    #[test]
    fn refusal_accepts_empty_string() {
        assert!(!is_llm_refusal(""));
    }

    #[test]
    fn refusal_detects_all_patterns() {
        let patterns = [
            "sorry about that",
            "i can't do this",
            "i cannot process",
            "i'm unable to help",
            "the transcription is bad",
            "appears to be noise",
            "seems to be garbled",
            "audio is truncated",
            "input is incomplete",
            "it's unintelligible",
            "not enough context here",
            "please provide more",
            "could you repeat that",
            "i apologize for",
            "as an ai model",
            "i'm an ai assistant",
        ];
        for p in &patterns {
            assert!(is_llm_refusal(p), "Should detect refusal in: '{}'", p);
        }
    }

    // ---- RefinementConfig tests ----

    #[test]
    fn default_config_has_expected_values() {
        let config = RefinementConfig::default();
        assert_eq!(config.api_url, "http://localhost:11434/v1/chat/completions");
        assert_eq!(config.api_key, "");
        assert_eq!(config.model, "llama3");
        assert!(!config.use_screenshot);
        assert_eq!(config.max_tokens, 2048);
        assert!(config.system_prompt.contains("{app_name}"));
        assert!(config.system_prompt.contains("{window_title}"));
        assert!(config.system_prompt.contains("{transcript}"));
    }

    #[test]
    fn default_prompt_contains_required_placeholders() {
        assert!(DEFAULT_SYSTEM_PROMPT.contains("{app_name}"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("{window_title}"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("{transcript}"));
    }

    #[test]
    fn default_prompt_contains_critical_rules() {
        assert!(DEFAULT_SYSTEM_PROMPT.contains("CRITICAL RULES"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("Output ONLY the cleaned text"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("NEVER complete, extend, or finish partial sentences"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("NEVER use the context"));
    }

    // ---- Prompt building tests (via mock server) ----

    fn make_context(app: &str, title: &str, text: &str, screenshot: Option<&str>) -> CapturedContext {
        CapturedContext {
            app_name: app.to_string(),
            window_title: title.to_string(),
            surrounding_text: text.to_string(),
            screenshot_base64: screenshot.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn refine_injects_app_name_and_window_title_into_prompt() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "Hello world." }
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("Code.exe", "main.rs - VS Code", "", None);

        let result = refiner.refine("hello world", &ctx).await.unwrap();
        assert_eq!(result, "Hello world.");

        // Verify the request body contained our context
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("Code.exe"), "Prompt should contain app name");
        assert!(content.contains("main.rs - VS Code"), "Prompt should contain window title");
        assert!(content.contains("hello world"), "Prompt should contain transcript");
    }

    #[tokio::test]
    async fn refine_substitutes_surrounding_text_placeholder() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined text" }
                }]
            })))
            .mount(&server)
            .await;

        // Use a custom prompt that includes {surrounding_text}
        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            system_prompt: "App: {app_name}\nTitle: {window_title}\nNearby: {surrounding_text}\nTranscript: {transcript}".to_string(),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("chrome.exe", "Google Docs", "some nearby text", None);

        refiner.refine("test input", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("some nearby text"), "Should substitute surrounding_text");
    }

    #[tokio::test]
    async fn refine_includes_screenshot_when_enabled() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            use_screenshot: true,
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", Some("dGVzdHNjcmVlbnNob3Q="));

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let content = &body["messages"][0]["content"];

        // Should be an array with text + image_url parts
        assert!(content.is_array(), "Content should be multipart array when screenshot enabled");
        assert_eq!(content.as_array().unwrap().len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert!(content[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    }

    #[tokio::test]
    async fn refine_skips_screenshot_when_disabled() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            use_screenshot: false,
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", Some("dGVzdHNjcmVlbnNob3Q="));

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let content = &body["messages"][0]["content"];

        // Should be a plain string, not an array
        assert!(content.is_string(), "Content should be plain string when screenshot disabled");
    }

    #[tokio::test]
    async fn refine_skips_screenshot_when_enabled_but_none_captured() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            use_screenshot: true,
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None); // No screenshot

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let content = &body["messages"][0]["content"];

        // Should fall back to plain string since no screenshot was captured
        assert!(content.is_string(), "Content should be plain string when no screenshot captured");
    }

    #[tokio::test]
    async fn refine_falls_back_to_raw_on_refusal() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "Sorry, I cannot process this transcription." }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        let result = refiner.refine("hello world", &ctx).await.unwrap();
        assert_eq!(result, "hello world", "Should fall back to raw transcript on refusal");
    }

    #[tokio::test]
    async fn refine_returns_error_on_api_failure() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        let result = refiner.refine("hello", &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn refine_sends_auth_header_when_api_key_set() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            api_key: "test-key-123".to_string(),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let auth = requests[0].headers.get("Authorization").unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer test-key-123");
    }

    #[tokio::test]
    async fn refine_omits_auth_header_when_api_key_empty() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            api_key: "".to_string(),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert!(requests[0].headers.get("Authorization").is_none());
    }

    #[tokio::test]
    async fn refine_sends_correct_model_and_temperature() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "content": "refined" }
                }]
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            model: "gpt-4o-mini".to_string(),
            max_tokens: 1024,
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        refiner.refine("hello", &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["temperature"], 0.3);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[tokio::test]
    async fn refine_falls_back_to_raw_when_no_choices() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": []
            })))
            .mount(&server)
            .await;

        let config = RefinementConfig {
            api_url: format!("{}/v1/chat/completions", server.uri()),
            ..Default::default()
        };
        let refiner = Refiner::new(config);
        let ctx = make_context("app.exe", "Window", "", None);

        let result = refiner.refine("raw text", &ctx).await.unwrap();
        assert_eq!(result, "raw text", "Should return raw transcript when no choices");
    }
}
