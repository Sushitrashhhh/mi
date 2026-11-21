use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "web.search".into(),
        description: "Search the web (not yet implemented).".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"}
            },
            "required": ["query"]
        }),
        summary_template: "Search: {query}".into(),
        taints: true,
    });

    registry.register(ToolDef {
        name: "web.fetch".into(),
        description: "Fetch a web page and return its text content (not yet implemented).".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "URL to fetch"}
            },
            "required": ["url"]
        }),
        summary_template: "Fetch {url}".into(),
        taints: true,
    });
}

pub async fn search(args: serde_json::Value) -> ToolResult {
    let query = args["query"].as_str().unwrap_or("");
    ToolResult::error(format!(
        "web.search is not yet implemented. Query: {}",
        query
    ))
}

pub async fn fetch(args: serde_json::Value) -> ToolResult {
    let url = args["url"].as_str().unwrap_or("");
    ToolResult::error(format!("web.fetch is not yet implemented. URL: {}", url))
}
