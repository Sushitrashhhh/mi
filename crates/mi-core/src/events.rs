use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub event_type: String,
    pub id: String,
    pub ts: DateTime<Utc>,
    pub session: String,
    pub data: serde_json::Value,
}

impl Envelope {
    pub fn new(event_type: &str, session: &str, data: serde_json::Value) -> Self {
        Self {
            event_type: event_type.to_string(),
            id: Uuid::new_v4().to_string(),
            ts: Utc::now(),
            session: session.to_string(),
            data,
        }
    }
}

// --- Client → Core ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub text: String,
    #[serde(default)]
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCancel {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub approval_id: String,
    pub decision: ApprovalDecision,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
    ApproveAlways,
}

// --- Core → Client ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantDelta {
    pub turn_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantDone {
    pub turn_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStarted {
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub args_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFinished {
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub result_preview: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub tool: String,
    pub tier: String,
    pub summary: String,
    pub args: serde_json::Value,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notice {
    pub level: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_creation() {
        let data = serde_json::json!({"text": "hello"});
        let env = Envelope::new("user.message", "sess-1", data);
        assert_eq!(env.event_type, "user.message");
        assert_eq!(env.session, "sess-1");
        assert!(!env.id.is_empty());
    }

    #[test]
    fn test_envelope_roundtrip() {
        let data = serde_json::json!({"text": "hi"});
        let env = Envelope::new("user.message", "sess-1", data);
        let json = serde_json::to_string(&env).unwrap();
        let parsed: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type, "user.message");
        assert_eq!(parsed.session, "sess-1");
    }

    #[test]
    fn test_approval_decision_serde() {
        let resp = ApprovalResponse {
            approval_id: "a1".into(),
            decision: ApprovalDecision::ApproveAlways,
            note: Some("ok".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("approve_always"));
        let parsed: ApprovalResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.decision, ApprovalDecision::ApproveAlways);
    }
}
