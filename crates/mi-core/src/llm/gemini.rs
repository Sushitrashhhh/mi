use std::pin::Pin;

use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::types::{Chunk, FunctionCall, Message, ModelInfo, Role};

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Debug, Error)]
pub enum GeminiError {
    #[error("no model configured — run `mi doctor --fix` or set [models] in ~/.mi/mi.toml")]
    NoModel,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub struct GeminiProvider {
    api_key: String,
    client: Client,
}

// --- Gemini API request types ---

#[derive(Serialize)]
struct GenerateRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "functionResponse")]
    function_response: Option<GeminiFunctionResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "inlineData")]
    inline_data: Option<InlineData>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "thought")]
    thought: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "thoughtSignature")]
    thought_signature: Option<String>,
}

impl Part {
    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            function_call: None,
            function_response: None,
            inline_data: None,
            thought: None,
            thought_signature: None,
        }
    }

    fn image(mime_type: String, data: String) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: None,
            inline_data: Some(InlineData { mime_type, data }),
            thought: None,
            thought_signature: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
struct Tool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<serde_json::Value>,
}

// --- Gemini API response types ---

#[derive(Deserialize, Debug)]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    #[serde(default)]
    content: Option<Content>,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    #[serde(default)]
    message: String,
}

// --- Models list response ---

#[derive(Deserialize, Debug)]
struct ModelsListResponse {
    #[serde(default)]
    models: Vec<ModelEntry>,
}

#[derive(Deserialize, Debug)]
struct ModelEntry {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "displayName")]
    display_name: String,
    #[serde(default, rename = "supportedGenerationMethods")]
    supported_generation_methods: Vec<String>,
    #[serde(default, rename = "inputTokenLimit")]
    input_token_limit: Option<u64>,
    #[serde(default, rename = "outputTokenLimit")]
    output_token_limit: Option<u64>,
}

impl GeminiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    fn message_to_content(msg: &Message) -> Content {
        match msg.role {
            Role::Tool => {
                let parts = msg
                    .function_responses
                    .iter()
                    .map(|fr| Part {
                        text: None,
                        function_call: None,
                        function_response: Some(GeminiFunctionResponse {
                            name: fr.name.clone(),
                            response: serde_json::json!({"result": fr.result}),
                        }),
                        inline_data: None,
                        thought: None,
                        thought_signature: None,
                    })
                    .collect();
                Content {
                    role: "user".to_string(),
                    parts,
                }
            }
            Role::Assistant if !msg.function_calls.is_empty() => {
                let mut parts = Vec::new();
                if !msg.text.is_empty() {
                    parts.push(Part::text(msg.text.clone()));
                }
                for fc in &msg.function_calls {
                    parts.push(Part {
                        text: None,
                        function_call: Some(GeminiFunctionCall {
                            name: fc.name.clone(),
                            args: fc.args.clone(),
                        }),
                        function_response: None,
                        inline_data: None,
                        thought: None,
                        thought_signature: fc.thought_signature.clone(),
                    });
                }
                Content {
                    role: "model".to_string(),
                    parts,
                }
            }
            Role::Assistant => Content {
                role: "model".to_string(),
                parts: vec![Part::text(msg.text.clone())],
            },
            Role::User => {
                let mut parts = Vec::new();
                for img in &msg.images {
                    parts.push(Part::image(img.mime_type.clone(), img.data.clone()));
                }
                // Always include a text part (the question / caption), even
                // when empty, so an image-only message still has a prompt slot.
                if !msg.text.is_empty() || parts.is_empty() {
                    parts.push(Part::text(msg.text.clone()));
                }
                Content {
                    role: "user".to_string(),
                    parts,
                }
            }
        }
    }

    pub fn stream(
        &self,
        messages: &[Message],
        model: &str,
        system: Option<&str>,
        tools: Option<&[serde_json::Value]>,
    ) -> Pin<Box<dyn Stream<Item = Result<Chunk, GeminiError>> + Send>> {
        if model.is_empty() {
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx.send(Err(GeminiError::NoModel)).await;
            });
            return Box::pin(ReceiverStream::new(rx));
        }

        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            BASE_URL, model, self.api_key
        );

        let contents: Vec<Content> = messages.iter().map(Self::message_to_content).collect();

        let system_instruction = system.map(|s| Content {
            role: "user".to_string(),
            parts: vec![Part::text(s.to_string())],
        });

        let tools_payload = tools.map(|decls| {
            vec![Tool {
                function_declarations: decls.to_vec(),
            }]
        });

        let request_body = GenerateRequest {
            contents,
            system_instruction,
            tools: tools_payload,
        };

        let client = self.client.clone();
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            let result = client.post(&url).json(&request_body).send().await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(GeminiError::Http(e))).await;
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let _ = tx
                    .send(Err(GeminiError::Api(format!(
                        "HTTP {}: {}",
                        status, body
                    ))))
                    .await;
                return;
            }

            let body = match response.text().await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(Err(GeminiError::Http(e))).await;
                    return;
                }
            };

            // Parse SSE response: lines starting with "data: " contain JSON
            for line in body.lines() {
                let line = line.trim();
                if let Some(json_str) = line.strip_prefix("data: ") {
                    let parsed: Result<GenerateResponse, _> = serde_json::from_str(json_str);
                    match parsed {
                        Ok(resp) => {
                            if let Some(err) = resp.error {
                                let _ = tx.send(Err(GeminiError::Api(err.message))).await;
                                return;
                            }
                            for candidate in &resp.candidates {
                                if let Some(content) = &candidate.content {
                                    let mut fc_list = Vec::new();
                                    let mut text = String::new();

                                    for part in &content.parts {
                                        if let Some(t) = &part.text {
                                            text.push_str(t);
                                        }
                                        if let Some(fc) = &part.function_call {
                                            fc_list.push(FunctionCall {
                                                name: fc.name.clone(),
                                                args: fc.args.clone(),
                                                call_id: fc.name.clone(),
                                                thought_signature: part.thought_signature.clone(),
                                            });
                                        }
                                    }

                                    if !fc_list.is_empty() {
                                        let _ = tx
                                            .send(Ok(Chunk {
                                                function_calls: fc_list,
                                                ..Default::default()
                                            }))
                                            .await;
                                    } else if !text.is_empty() {
                                        let _ = tx
                                            .send(Ok(Chunk {
                                                text,
                                                ..Default::default()
                                            }))
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // Skip non-JSON SSE lines (e.g., empty events)
                        }
                    }
                }
            }

            let _ = tx
                .send(Ok(Chunk {
                    done: true,
                    ..Default::default()
                }))
                .await;
        });

        Box::pin(ReceiverStream::new(rx))
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, GeminiError> {
        let url = format!("{}/models?key={}&pageSize=1000", BASE_URL, self.api_key);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GeminiError::Api(format!("HTTP {}: {}", status, body)));
        }

        let resp: ModelsListResponse = response.json().await?;

        let mut models: Vec<ModelInfo> = resp
            .models
            .into_iter()
            .map(|m| {
                let id = m.name.strip_prefix("models/").unwrap_or(&m.name).to_string();
                ModelInfo {
                    id,
                    display_name: m.display_name,
                    supported_methods: m.supported_generation_methods,
                    input_token_limit: m.input_token_limit,
                    output_token_limit: m.output_token_limit,
                }
            })
            .collect();

        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_to_content_user() {
        let msg = Message::user("hello");
        let content = GeminiProvider::message_to_content(&msg);
        assert_eq!(content.role, "user");
        assert_eq!(content.parts[0].text.as_deref(), Some("hello"));
    }

    #[test]
    fn test_message_to_content_assistant() {
        let msg = Message::assistant("hi there");
        let content = GeminiProvider::message_to_content(&msg);
        assert_eq!(content.role, "model");
        assert_eq!(content.parts[0].text.as_deref(), Some("hi there"));
    }

    #[test]
    fn test_message_to_content_tool_response() {
        let msg = Message::tool_response("call-1", "system.time", "12:00 PM");
        let content = GeminiProvider::message_to_content(&msg);
        assert_eq!(content.role, "user");
        assert!(content.parts[0].function_response.is_some());
    }

    #[test]
    fn test_message_to_content_assistant_with_function_calls() {
        let msg = Message {
            role: Role::Assistant,
            text: String::new(),
            function_calls: vec![FunctionCall::new("system.time", serde_json::json!({}), "c1")],
            function_responses: vec![],
            images: vec![],
        };
        let content = GeminiProvider::message_to_content(&msg);
        assert_eq!(content.role, "model");
        assert!(content.parts[0].function_call.is_some());
    }
}
