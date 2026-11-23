use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub call_id: String,
    pub name: String,
    pub result: String,
}

/// An image attached to a message, sent to the model as `inline_data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    /// e.g. "image/png"
    pub mime_type: String,
    /// base64-encoded image bytes
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub function_calls: Vec<FunctionCall>,
    #[serde(default)]
    pub function_responses: Vec<FunctionResponse>,
    #[serde(default)]
    pub images: Vec<ImageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub text: String,
    pub function_calls: Vec<FunctionCall>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub supported_methods: Vec<String>,
    #[serde(default)]
    pub input_token_limit: Option<u64>,
    #[serde(default)]
    pub output_token_limit: Option<u64>,
}

impl FunctionCall {
    pub fn new(name: &str, args: serde_json::Value, call_id: &str) -> Self {
        Self {
            name: name.to_string(),
            args,
            call_id: call_id.to_string(),
            thought_signature: None,
        }
    }
}

impl Message {
    pub fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            text: text.to_string(),
            function_calls: vec![],
            function_responses: vec![],
            images: vec![],
        }
    }

    pub fn user_with_images(text: &str, images: Vec<ImageData>) -> Self {
        Self {
            role: Role::User,
            text: text.to_string(),
            function_calls: vec![],
            function_responses: vec![],
            images,
        }
    }

    pub fn assistant(text: &str) -> Self {
        Self {
            role: Role::Assistant,
            text: text.to_string(),
            function_calls: vec![],
            function_responses: vec![],
            images: vec![],
        }
    }

    pub fn tool_response(call_id: &str, name: &str, result: &str) -> Self {
        Self {
            role: Role::Tool,
            text: String::new(),
            function_calls: vec![],
            function_responses: vec![FunctionResponse {
                call_id: call_id.to_string(),
                name: name.to_string(),
                result: result.to_string(),
            }],
            images: vec![],
        }
    }
}
