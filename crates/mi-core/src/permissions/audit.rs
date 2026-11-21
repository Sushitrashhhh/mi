use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub ts: DateTime<Utc>,
    pub session: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn append(&self, entry: &AuditEntry) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let line = match serde_json::to_string(entry) {
            Ok(l) => l,
            Err(_) => return,
        };
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{}", line);
        }
    }

    pub fn log_tool_call(
        &self,
        session: &str,
        tool: &str,
        args: &serde_json::Value,
        result_ok: bool,
        result_preview: &str,
    ) {
        self.append(&AuditEntry {
            ts: Utc::now(),
            session: session.into(),
            event_type: "tool_call".into(),
            tool: Some(tool.into()),
            args: Some(args.clone()),
            result_ok: Some(result_ok),
            result_preview: Some(result_preview.chars().take(200).collect()),
            decision: None,
            model: None,
        });
    }

    pub fn log_approval(&self, session: &str, tool: &str, decision: &str) {
        self.append(&AuditEntry {
            ts: Utc::now(),
            session: session.into(),
            event_type: "approval".into(),
            tool: Some(tool.into()),
            args: None,
            result_ok: None,
            result_preview: None,
            decision: Some(decision.into()),
            model: None,
        });
    }

    pub fn log_llm_call(&self, session: &str, model: &str) {
        self.append(&AuditEntry {
            ts: Utc::now(),
            session: session.into(),
            event_type: "llm_call".into(),
            tool: None,
            args: None,
            result_ok: None,
            result_preview: None,
            decision: None,
            model: Some(model.into()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::new(path.clone());

        log.log_tool_call("sess-1", "system.time", &serde_json::json!({}), true, "12:00 PM");
        log.log_approval("sess-1", "files.delete", "Approve");
        log.log_llm_call("sess-1", "gemini-2.0-flash");

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("tool_call"));
        assert!(lines[1].contains("approval"));
        assert!(lines[2].contains("llm_call"));
    }
}
