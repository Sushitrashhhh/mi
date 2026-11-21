use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

#[derive(Debug, Clone)]
pub struct Timer {
    pub id: String,
    pub label: String,
    pub fires_at: DateTime<Utc>,
}

pub type TimerStore = HashMap<String, Timer>;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "timers.set".into(),
        description: "Set a timer or reminder. Specify duration in seconds.".into(),
        tier: Tier::Safe,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "label": {"type": "string", "description": "What the timer is for"},
                "seconds": {"type": "number", "description": "Duration in seconds"}
            },
            "required": ["label", "seconds"]
        }),
        summary_template: "Set timer: {label}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "timers.list".into(),
        description: "List all active timers.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({"type": "object", "properties": {}}),
        summary_template: "List timers".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "timers.cancel".into(),
        description: "Cancel a timer by ID.".into(),
        tier: Tier::Safe,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Timer ID to cancel"}
            },
            "required": ["id"]
        }),
        summary_template: "Cancel timer {id}".into(),
        taints: false,
    });
}

pub async fn set(args: serde_json::Value, store: &Arc<Mutex<TimerStore>>) -> ToolResult {
    let label = args["label"].as_str().unwrap_or("timer");
    let seconds = args["seconds"].as_f64().unwrap_or(0.0);
    if seconds <= 0.0 {
        return ToolResult::error("seconds must be positive");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let fires_at = Utc::now() + chrono::Duration::seconds(seconds as i64);
    let timer = Timer {
        id: id.clone(),
        label: label.to_string(),
        fires_at,
    };
    store.lock().unwrap().insert(id.clone(), timer);
    ToolResult::ok(format!(
        "Timer '{}' set for {} seconds (id: {})",
        label, seconds, id
    ))
}

pub async fn list(store: &Arc<Mutex<TimerStore>>) -> ToolResult {
    let timers = store.lock().unwrap();
    if timers.is_empty() {
        return ToolResult::ok("No active timers.");
    }
    let now = Utc::now();
    let lines: Vec<String> = timers
        .values()
        .map(|t| {
            let remaining = (t.fires_at - now).num_seconds().max(0);
            format!("[{}] {} — {}s remaining", t.id, t.label, remaining)
        })
        .collect();
    ToolResult::ok(lines.join("\n"))
}

pub async fn cancel(args: serde_json::Value, store: &Arc<Mutex<TimerStore>>) -> ToolResult {
    let id = args["id"].as_str().unwrap_or("");
    if id.is_empty() {
        return ToolResult::error("id is required");
    }
    let mut timers = store.lock().unwrap();
    if timers.remove(id).is_some() {
        ToolResult::ok(format!("Timer {} cancelled", id))
    } else {
        ToolResult::error(format!("Timer {} not found", id))
    }
}
