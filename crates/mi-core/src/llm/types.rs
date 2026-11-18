use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub call_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub call_id: String,
    pub name: String,
    pub result: String,
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

impl Message {
    pub fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            text: text.to_string(),
            function_calls: vec![],
            function_responses: vec![],
        }
    }

    pub fn assistant(text: &str) -> Self {
        Self {
            role: Role::Assistant,
            text: text.to_string(),
            function_calls: vec![],
            function_responses: vec![],
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
        }
    }
}
