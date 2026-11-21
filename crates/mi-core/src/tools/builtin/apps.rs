use crate::tools::os_adapt::OsAdapter;
use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "apps.list".into(),
        description: "List currently running applications with visible windows.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({"type": "object", "properties": {}}),
        summary_template: "List running apps".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "apps.open".into(),
        description: "Open an application or URL. For URLs, opens in the default browser.".into(),
        tier: Tier::Safe,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {"type": "string", "description": "Application name, path, or URL to open"}
            },
            "required": ["target"]
        }),
        summary_template: "Open {target}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "apps.focus".into(),
        description: "Bring a window to the foreground by partial title match.".into(),
        tier: Tier::Safe,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Partial window title to match"}
            },
            "required": ["title"]
        }),
        summary_template: "Focus window: {title}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "apps.close".into(),
        description: "Close an application by process name.".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Process name to close"}
            },
            "required": ["name"]
        }),
        summary_template: "Close {name}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "apps.type_text".into(),
        description: "Type text into the currently focused application window.".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Text to type"}
            },
            "required": ["text"]
        }),
        summary_template: "Type into focused app".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "apps.hotkey".into(),
        description: "Send a keyboard shortcut (e.g., ctrl+c, alt+tab).".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "keys": {"type": "string", "description": "Hotkey combination, e.g. 'ctrl+c'"}
            },
            "required": ["keys"]
        }),
        summary_template: "Send hotkey: {keys}".into(),
        taints: false,
    });
}

pub async fn list(os: &dyn OsAdapter) -> ToolResult {
    match os.list_windows().await {
        Ok(windows) => {
            if windows.is_empty() {
                return ToolResult::ok("No visible windows found.");
            }
            let lines: Vec<String> = windows
                .iter()
                .map(|w| format!("{} — {}", w.process_name, w.title))
                .collect();
            ToolResult::ok(lines.join("\n"))
        }
        Err(e) => ToolResult::error(e),
    }
}

pub async fn open(args: serde_json::Value, os: &dyn OsAdapter) -> ToolResult {
    let target = args["target"].as_str().unwrap_or("");
    if target.is_empty() {
        return ToolResult::error("target is required");
    }
    if target.starts_with("http://") || target.starts_with("https://") || target.contains("://") {
        match os.open_url(target).await {
            Ok(msg) => ToolResult::ok(msg),
            Err(e) => ToolResult::error(e),
        }
    } else {
        match os.open_app(target, &[]).await {
            Ok(msg) => ToolResult::ok(msg),
            Err(e) => ToolResult::error(e),
        }
    }
}

pub async fn focus(args: serde_json::Value, os: &dyn OsAdapter) -> ToolResult {
    let title = args["title"].as_str().unwrap_or("");
    if title.is_empty() {
        return ToolResult::error("title is required");
    }
    match os.focus_window(title).await {
        Ok(msg) => ToolResult::ok(msg),
        Err(e) => ToolResult::error(e),
    }
}

pub async fn close(args: serde_json::Value, os: &dyn OsAdapter) -> ToolResult {
    let name = args["name"].as_str().unwrap_or("");
    if name.is_empty() {
        return ToolResult::error("name is required");
    }
    match os.close_app(name).await {
        Ok(msg) => ToolResult::ok(msg),
        Err(e) => ToolResult::error(e),
    }
}

pub async fn type_text(args: serde_json::Value, os: &dyn OsAdapter) -> ToolResult {
    let text = args["text"].as_str().unwrap_or("");
    if text.is_empty() {
        return ToolResult::error("text is required");
    }
    match os.type_text(text).await {
        Ok(msg) => ToolResult::ok(msg),
        Err(e) => ToolResult::error(e),
    }
}

pub async fn hotkey(args: serde_json::Value, os: &dyn OsAdapter) -> ToolResult {
    let keys = args["keys"].as_str().unwrap_or("");
    if keys.is_empty() {
        return ToolResult::error("keys is required");
    }
    match os.send_hotkey(keys).await {
        Ok(msg) => ToolResult::ok(msg),
        Err(e) => ToolResult::error(e),
    }
}
