use crate::tools::registry::{Tier, ToolDef, ToolRegistry, ToolResult};

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolDef {
        name: "shell.run".into(),
        description: "Run a shell command and return its output. Commands run with cmd /C on Windows.".into(),
        tier: Tier::Confirm,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The shell command to run"},
                "cwd": {"type": "string", "description": "Working directory (optional)"}
            },
            "required": ["command"]
        }),
        summary_template: "Run: {command}".into(),
        taints: false,
    });
}

pub async fn run(args: serde_json::Value) -> ToolResult {
    let command = args["command"].as_str().unwrap_or("");
    if command.is_empty() {
        return ToolResult::error("command is required");
    }
    let cwd = args["cwd"].as_str();

    let mut cmd = tokio::process::Command::new("cmd");
    cmd.args(["/C", command]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    match tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str("[stderr] ");
                result.push_str(&stderr);
            }
            if result.len() > 10_000 {
                result.truncate(10_000);
                result.push_str("\n[output truncated]");
            }
            if output.status.success() {
                ToolResult::ok(if result.is_empty() {
                    "(no output)".into()
                } else {
                    result
                })
            } else {
                ToolResult::error(format!("exit {}: {}", output.status, result))
            }
        }
        Ok(Err(e)) => ToolResult::error(format!("failed to run command: {}", e)),
        Err(_) => ToolResult::error("command timed out after 30s"),
    }
}
