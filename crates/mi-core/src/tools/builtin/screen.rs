use crate::llm::types::ImageData;
use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};
use crate::vision::{capture, CaptureTarget};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "screen.capture".into(),
        description: "Capture a screenshot of the user's screen so you can see what they're \
            looking at (e.g. to read an on-screen error). The image is attached to the \
            conversation for you to analyze. Prefer target \"window\" for a specific app, \
            or \"screen\" for the whole display."
            .into(),
        tier: Tier::Read,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "enum": ["screen", "window"],
                    "description": "What to capture: \"screen\" (primary monitor) or \"window\" (active window). Defaults to screen."
                }
            }
        }),
        summary_template: "Capture the screen".into(),
        // Screen content is untrusted input — taint the turn so risky tools
        // escalate to CONFIRM afterwards.
        taints: true,
    });
}

pub async fn capture_tool(args: serde_json::Value) -> ToolResult {
    let target = match args["target"].as_str() {
        Some("window") => CaptureTarget::ActiveWindow,
        _ => CaptureTarget::PrimaryMonitor,
    };

    let result = tokio::task::spawn_blocking(move || capture::capture(target)).await;

    let shot = match result {
        Ok(Ok(shot)) => shot,
        Ok(Err(e)) => return ToolResult::error(format!("screen capture failed: {}", e)),
        Err(e) => return ToolResult::error(format!("screen capture task error: {}", e)),
    };

    let image = ImageData {
        mime_type: shot.mime_type().to_string(),
        data: shot.to_base64(),
    };

    ToolResult::ok_with_images(
        format!(
            "Captured a {}x{} screenshot (attached below).",
            shot.width, shot.height
        ),
        vec![image],
    )
}
