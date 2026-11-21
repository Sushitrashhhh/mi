use crate::tools::os_adapt::OsAdapter;
use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "system.time".into(),
        description: "Get the current date and time.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        summary_template: "Get current time".into(),
        taints: false,
    });

    registry.register(ToolDef {
        name: "system.info".into(),
        description: "Get system information: OS, CPU, RAM.".into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        summary_template: "Get system info".into(),
        taints: false,
    });
}

pub fn time() -> ToolResult {
    let now = chrono::Local::now();
    ToolResult::ok(now.format("%A, %B %d %Y, %I:%M:%S %p").to_string())
}

pub async fn info(os: &dyn OsAdapter) -> ToolResult {
    match os.system_info().await {
        Ok(info) => ToolResult::ok(info),
        Err(e) => ToolResult::error(e),
    }
}
