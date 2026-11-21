use std::sync::Arc;

use regex::Regex;
use serde::Deserialize;

use crate::config::AutoSave;
use crate::llm::AnyProvider;
use crate::llm::types::Message;

use super::embed::EmbeddingClient;
use super::store::{Memory, MemoryKind, MemorySource, MemoryStore};

pub struct MemoryExtractor {
    provider: Arc<AnyProvider>,
    model: String,
    store: Arc<MemoryStore>,
    embedder: Arc<EmbeddingClient>,
    auto_save: AutoSave,
}

#[derive(Debug, Deserialize)]
struct ExtractedMemory {
    kind: String,
    text: String,
}

const EXTRACTION_PROMPT_STATED_ONLY: &str = r#"Extract ONLY facts, preferences, or personal details the user **explicitly stated** about themselves in this conversation turn. Do NOT infer or guess.

Return a JSON array. Each element: {"kind": "fact"|"preference"|"person"|"task", "text": "<the memory>"}
If nothing worth remembering was explicitly stated, return [].

Rules:
- Only extract what the user directly said about themselves, their preferences, or people they mentioned
- Do NOT extract the question they asked or the topic of conversation
- Do NOT extract temporary/ephemeral info (e.g. "open this file")
- Do NOT extract secrets, passwords, API keys, or sensitive numbers
- State each memory as a third-person fact (e.g. "User edits in Premiere Pro")

Conversation turn:
"#;

const EXTRACTION_PROMPT_ALL: &str = r#"Extract facts, preferences, personal details, tasks, and notable information from this conversation turn.

Return a JSON array. Each element: {"kind": "fact"|"preference"|"person"|"task"|"episode", "text": "<the memory>"}
If nothing worth remembering, return [].

Rules:
- Extract what the user revealed about themselves, their work, preferences, or people
- Extract tasks they mentioned or committed to
- Do NOT extract the specific question asked unless it reveals a lasting preference
- Do NOT extract secrets, passwords, API keys, or sensitive numbers
- State each memory as a third-person fact (e.g. "User edits in Premiere Pro")

Conversation turn:
"#;

impl MemoryExtractor {
    pub fn new(
        provider: Arc<AnyProvider>,
        model: String,
        store: Arc<MemoryStore>,
        embedder: Arc<EmbeddingClient>,
        auto_save: AutoSave,
    ) -> Self {
        Self {
            provider,
            model,
            store,
            embedder,
            auto_save,
        }
    }

    /// Extract memories from the most recent user+assistant turn.
    /// Returns the number of memories saved.
    pub async fn extract_from_turn(
        &self,
        user_text: &str,
        assistant_text: &str,
    ) -> anyhow::Result<usize> {
        if self.auto_save == AutoSave::Off {
            return Ok(0);
        }

        let prompt = match self.auto_save {
            AutoSave::StatedOnly => EXTRACTION_PROMPT_STATED_ONLY,
            AutoSave::All => EXTRACTION_PROMPT_ALL,
            AutoSave::Off => return Ok(0),
        };

        let extraction_input = format!(
            "{}User: {}\nAssistant: {}",
            prompt, user_text, assistant_text
        );

        let messages = vec![Message::user(&extraction_input)];
        let result = self
            .provider
            .generate(&messages, &self.model, None, None)
            .await;

        let response_text = match result {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("memory extraction LLM call failed: {}", e);
                return Ok(0);
            }
        };

        let extracted = parse_extraction_response(&response_text);
        let mut saved = 0;

        for item in extracted {
            let kind = match item.kind.as_str() {
                "fact" => MemoryKind::Fact,
                "preference" => MemoryKind::Preference,
                "episode" => MemoryKind::Episode,
                "task" => MemoryKind::Task,
                "person" => MemoryKind::Person,
                _ => MemoryKind::Fact,
            };

            let clean_text = filter_secrets(&item.text);
            if clean_text.trim().is_empty() || clean_text.contains("[REDACTED]") {
                continue;
            }

            let now = chrono::Utc::now().to_rfc3339();
            let memory = Memory {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                text: clean_text.clone(),
                source: MemorySource::Extracted,
                confidence: 0.8,
                created_at: now.clone(),
                updated_at: now,
                last_used_at: None,
                pinned: false,
            };

            if let Err(e) = self.store.save_memory(&memory) {
                tracing::warn!("failed to save extracted memory: {}", e);
                continue;
            }

            // Embed and store vector (best-effort)
            match self.embedder.embed(&memory.text).await {
                Ok(embedding) => {
                    let _ = self.store.save_embedding(&memory.id, &embedding);
                }
                Err(e) => {
                    tracing::warn!("failed to embed memory: {}", e);
                }
            }

            saved += 1;
        }

        Ok(saved)
    }
}

fn parse_extraction_response(text: &str) -> Vec<ExtractedMemory> {
    // The LLM should return a JSON array, but may wrap it in markdown code fences
    let trimmed = text.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    // Try to find a JSON array in the response
    if let Some(start) = json_str.find('[')
        && let Some(end) = json_str.rfind(']')
    {
        let slice = &json_str[start..=end];
        if let Ok(items) = serde_json::from_str::<Vec<ExtractedMemory>>(slice) {
            return items;
        }
    }

    vec![]
}

/// Filter secrets from text before storing in memory.
/// Replaces patterns that look like API keys, passwords, card numbers, SSNs.
pub fn filter_secrets(text: &str) -> String {
    let mut result = text.to_string();

    // API key patterns (Google AIza..., OpenAI sk-..., GitHub ghp_...)
    let api_key_re = Regex::new(r"(?i)(AIza[A-Za-z0-9_-]{30,}|sk-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|gh[pousr]_[A-Za-z0-9]{30,})").unwrap();
    result = api_key_re.replace_all(&result, "[REDACTED]").to_string();

    // Generic long token-like strings after key/token/secret/password words
    let token_re = Regex::new(r#"(?i)(key|token|secret|password|api_key|apikey)\s*[:=]\s*['"]?([A-Za-z0-9_+/=-]{20,})['"]?"#).unwrap();
    result = token_re.replace_all(&result, "$1: [REDACTED]").to_string();

    // Credit card numbers (4 groups of 4 digits)
    let cc_re = Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap();
    result = cc_re.replace_all(&result, "[REDACTED]").to_string();

    // SSN pattern
    let ssn_re = Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap();
    result = ssn_re.replace_all(&result, "[REDACTED]").to_string();

    // "my password is ..." / "my pin is ..."
    let pw_re = Regex::new(r"(?i)(password|pin|passcode)\s+(is|was|:)\s+\S+").unwrap();
    result = pw_re.replace_all(&result, "$1 $2 [REDACTED]").to_string();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_secrets_api_keys() {
        let text = "My key is AIzaSyD1234567890abcdefghijklmnopqrstuv";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("AIza"));
    }

    #[test]
    fn test_filter_secrets_openai_key() {
        let text = "token: sk-abcdefghijklmnopqrstuvwxyz1234";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("sk-"));
    }

    #[test]
    fn test_filter_secrets_credit_card() {
        let text = "My card is 4111-1111-1111-1111";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("4111"));
    }

    #[test]
    fn test_filter_secrets_ssn() {
        let text = "SSN: 123-45-6789";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("123-45"));
    }

    #[test]
    fn test_filter_secrets_password() {
        let text = "my password is hunter2";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("hunter2"));
    }

    #[test]
    fn test_filter_secrets_preserves_normal_text() {
        let text = "User edits video in Premiere Pro and prefers short answers";
        let filtered = filter_secrets(text);
        assert_eq!(filtered, text);
    }

    #[test]
    fn test_filter_secrets_github_token() {
        let text = "export GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh";
        let filtered = filter_secrets(text);
        assert!(filtered.contains("[REDACTED]"));
        assert!(!filtered.contains("ghp_"));
    }

    #[test]
    fn test_parse_extraction_response_valid() {
        let resp = r#"[{"kind": "preference", "text": "User likes short answers"}]"#;
        let items = parse_extraction_response(resp);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "preference");
        assert_eq!(items[0].text, "User likes short answers");
    }

    #[test]
    fn test_parse_extraction_response_code_fence() {
        let resp = "```json\n[{\"kind\": \"fact\", \"text\": \"User edits in Premiere\"}]\n```";
        let items = parse_extraction_response(resp);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_parse_extraction_response_empty() {
        let resp = "[]";
        let items = parse_extraction_response(resp);
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_extraction_response_garbage() {
        let resp = "I didn't find anything to extract.";
        let items = parse_extraction_response(resp);
        assert!(items.is_empty());
    }
}
