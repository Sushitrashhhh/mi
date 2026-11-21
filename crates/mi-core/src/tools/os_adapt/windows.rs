use std::future::Future;
use std::pin::Pin;

use super::{OsAdapter, WindowInfo};

pub struct WindowsAdapter;

impl WindowsAdapter {
    async fn powershell(script: &str) -> Result<String, String> {
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("exit {}: {}", output.status, stderr.trim()))
        }
    }
}

impl OsAdapter for WindowsAdapter {
    fn open_url(
        &self,
        url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let url = url.to_string();
        Box::pin(async move {
            let output = tokio::process::Command::new("cmd")
                .args(["/C", "start", "", &url])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(format!("Opened URL: {}", url))
            } else {
                Err(format!("Failed to open URL: {}", url))
            }
        })
    }

    fn open_app(
        &self,
        app: &str,
        args: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let app = app.to_string();
        let args: Vec<String> = args.to_vec();
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(&app);
            for arg in &args {
                cmd.arg(arg);
            }
            cmd.spawn().map_err(|e| e.to_string())?;
            Ok(format!("Opened: {}", app))
        })
    }

    fn list_windows(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<WindowInfo>, String>> + Send>> {
        Box::pin(async {
            let out = WindowsAdapter::powershell(
                "Get-Process | Where-Object {$_.MainWindowTitle -ne ''} | Select-Object ProcessName, MainWindowTitle | ConvertTo-Json",
            )
            .await?;
            if out.is_empty() {
                return Ok(Vec::new());
            }
            let parsed: Vec<serde_json::Value> = serde_json::from_str(&out)
                .or_else(|_| serde_json::from_str(&format!("[{}]", out)))
                .unwrap_or_default();
            let windows = parsed
                .iter()
                .map(|v| WindowInfo {
                    process_name: v["ProcessName"].as_str().unwrap_or("").to_string(),
                    title: v["MainWindowTitle"].as_str().unwrap_or("").to_string(),
                })
                .collect();
            Ok(windows)
        })
    }

    fn focus_window(
        &self,
        title: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let title = title.to_string();
        Box::pin(async move {
            let script = format!(
                r#"$w = Get-Process | Where-Object {{$_.MainWindowTitle -like '*{}*'}} | Select-Object -First 1; if ($w) {{ Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.Interaction]::AppActivate($w.Id) }} else {{ Write-Error 'Window not found' }}"#,
                title.replace('\'', "''")
            );
            WindowsAdapter::powershell(&script).await?;
            Ok(format!("Focused window matching: {}", title))
        })
    }

    fn close_app(
        &self,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let name = name.to_string();
        Box::pin(async move {
            let script = format!(
                "Get-Process -Name '{}' -ErrorAction SilentlyContinue | ForEach-Object {{ $_.CloseMainWindow() | Out-Null }}",
                name.replace('\'', "''")
            );
            WindowsAdapter::powershell(&script).await?;
            Ok(format!("Closed: {}", name))
        })
    }

    fn type_text(
        &self,
        text: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let text = text.to_string();
        Box::pin(async move {
            let escaped = text
                .replace('{', "{{")
                .replace('}', "}}")
                .replace('\'', "''");
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')",
                escaped
            );
            WindowsAdapter::powershell(&script).await?;
            Ok("Typed text".into())
        })
    }

    fn send_hotkey(
        &self,
        keys: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        let keys = keys.to_string();
        Box::pin(async move {
            let sendkeys = keys
                .replace("ctrl+", "^")
                .replace("alt+", "%")
                .replace("shift+", "+");
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')",
                sendkeys
            );
            WindowsAdapter::powershell(&script).await?;
            Ok(format!("Sent hotkey: {}", keys))
        })
    }

    fn system_info(&self) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
        Box::pin(async {
            let script = r#"$os = Get-CimInstance Win32_OperatingSystem; $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1; "OS: $($os.Caption) $($os.Version)`nCPU: $($cpu.Name)`nRAM: $([math]::Round($os.TotalVisibleMemorySize/1MB, 1)) GB total, $([math]::Round($os.FreePhysicalMemory/1MB, 1)) GB free""#;
            WindowsAdapter::powershell(script).await
        })
    }
}
