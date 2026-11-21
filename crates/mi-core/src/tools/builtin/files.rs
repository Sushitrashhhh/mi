use std::path::Path;

use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "files.list".into(),
        description: "List files and folders in a directory.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path to list"}
            },
            "required": ["path"]
        }),
        summary_template: "List files in {path}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "files.read".into(),
        description: "Read the contents of a text file.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to read"}
            },
            "required": ["path"]
        }),
        summary_template: "Read {path}".into(),
        taints: true,
    });

    registry.register(ToolDef {
        name: "files.create".into(),
        description: "Create a new file with the given content.".into(),
        tier: Tier::Safe,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to create"},
                "content": {"type": "string", "description": "File content"}
            },
            "required": ["path", "content"]
        }),
        summary_template: "Create file {path}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "files.move".into(),
        description: "Move or rename a file or folder.".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "src": {"type": "string", "description": "Source path"},
                "dst": {"type": "string", "description": "Destination path"}
            },
            "required": ["src", "dst"]
        }),
        summary_template: "Move {src} → {dst}".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "files.delete".into(),
        description: "Delete a file by moving it to quarantine (restorable for 14 days).".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path to delete"}
            },
            "required": ["path"]
        }),
        summary_template: "Delete {path}".into(),
        taints: false,
    });
}

pub async fn list(args: serde_json::Value) -> ToolResult {
    let path = args["path"].as_str().unwrap_or(".");
    let dir = Path::new(path);
    if !dir.is_dir() {
        return ToolResult::error(format!("not a directory: {}", path));
    }
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let mut items: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let suffix = entry
                    .file_type()
                    .map(|t| if t.is_dir() { "/" } else { "" })
                    .unwrap_or("");
                items.push(format!("{}{}", name, suffix));
            }
            items.sort();
            ToolResult::ok(items.join("\n"))
        }
        Err(e) => ToolResult::error(e.to_string()),
    }
}

pub async fn read(args: serde_json::Value) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");
    if path.is_empty() {
        return ToolResult::error("path is required");
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            if content.len() > 10_000 {
                ToolResult::ok(format!(
                    "{}...\n[truncated, {} bytes total]",
                    &content[..10_000],
                    content.len()
                ))
            } else {
                ToolResult::ok(content)
            }
        }
        Err(e) => ToolResult::error(e.to_string()),
    }
}

pub async fn create(args: serde_json::Value) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");
    let content = args["content"].as_str().unwrap_or("");
    if path.is_empty() {
        return ToolResult::error("path is required");
    }
    match std::fs::write(path, content) {
        Ok(()) => ToolResult::ok(format!("Created {}", path)),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

pub async fn r#move(args: serde_json::Value) -> ToolResult {
    let src = args["src"].as_str().unwrap_or("");
    let dst = args["dst"].as_str().unwrap_or("");
    if src.is_empty() || dst.is_empty() {
        return ToolResult::error("src and dst are required");
    }
    match std::fs::rename(src, dst) {
        Ok(()) => ToolResult::ok(format!("Moved {} → {}", src, dst)),
        Err(e) => ToolResult::error(e.to_string()),
    }
}

pub async fn delete(args: serde_json::Value, quarantine_dir: &Path) -> ToolResult {
    let path_str = args["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return ToolResult::error("path is required");
    }
    let path = Path::new(path_str);
    if !path.exists() {
        return ToolResult::error(format!("file not found: {}", path_str));
    }
    if let Err(e) = std::fs::create_dir_all(quarantine_dir) {
        return ToolResult::error(format!("cannot create quarantine dir: {}", e));
    }
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let quarantine_name = format!("{}_{}", ts, file_name);
    let quarantine_path = quarantine_dir.join(&quarantine_name);

    match std::fs::rename(path, &quarantine_path) {
        Ok(()) => ToolResult::ok(format!(
            "Deleted {} (quarantined as {}, restorable for 14 days)",
            path_str, quarantine_name
        )),
        Err(e) => ToolResult::error(format!("failed to quarantine: {}", e)),
    }
}
