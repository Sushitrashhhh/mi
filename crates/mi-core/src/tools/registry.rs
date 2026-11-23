use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::llm::types::ImageData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tier {
    Read,
    Safe,
    Confirm,
    Blocked,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Read => write!(f, "READ"),
            Tier::Safe => write!(f, "SAFE"),
            Tier::Confirm => write!(f, "CONFIRM"),
            Tier::Blocked => write!(f, "BLOCKED"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub tier: Tier,
    pub parameters: serde_json::Value,
    pub summary_template: String,
    pub taints: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub content: String,
    /// Images produced by the tool (e.g. a screenshot), injected into the
    /// conversation as a follow-up user message so the model can see them.
    #[serde(default)]
    pub images: Vec<ImageData>,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            images: vec![],
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
            images: vec![],
        }
    }

    pub fn ok_with_images(content: impl Into<String>, images: Vec<ImageData>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            images,
        }
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolDef>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, def: ToolDef) {
        self.tools.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name)
    }

    pub fn gemini_declarations(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|def| {
                serde_json::json!({
                    "name": def.name,
                    "description": def.description,
                    "parameters": def.parameters,
                })
            })
            .collect()
    }
}

pub fn render_summary(template: &str, args: &serde_json::Value) -> String {
    let mut result = template.to_string();
    if let Some(obj) = args.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{}}}", key);
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_display() {
        assert_eq!(Tier::Read.to_string(), "READ");
        assert_eq!(Tier::Confirm.to_string(), "CONFIRM");
    }

    #[test]
    fn test_tool_result() {
        let ok = ToolResult::ok("hello");
        assert!(ok.ok);
        assert_eq!(ok.content, "hello");

        let err = ToolResult::error("fail");
        assert!(!err.ok);
    }

    #[test]
    fn test_registry_and_declarations() {
        let mut reg = ToolRegistry::new();
        reg.register(ToolDef {
            name: "system.time".into(),
            description: "Get current time".into(),
            tier: Tier::Read,
            parameters: serde_json::json!({"type": "object", "properties": {}}),
            summary_template: "Get current time".into(),
            taints: false,
        });

        assert!(reg.get("system.time").is_some());
        assert!(reg.get("system.foo").is_none());

        let decls = reg.gemini_declarations();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "system.time");
    }

    #[test]
    fn test_render_summary() {
        let summary =
            render_summary("Delete {path}", &serde_json::json!({"path": "old.txt"}));
        assert_eq!(summary, "Delete old.txt");

        let summary = render_summary(
            "Move {src} → {dst}",
            &serde_json::json!({"src": "a.txt", "dst": "b.txt"}),
        );
        assert_eq!(summary, "Move a.txt → b.txt");
    }
}
