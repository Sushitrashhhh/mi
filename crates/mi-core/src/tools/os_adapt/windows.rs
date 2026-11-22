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
            let app_lower = app.to_lowercase();

            // Check for well-known protocol URIs first (opens UWP/Store apps)
            let protocol = match app_lower.as_str() {
                "whatsapp" => Some("whatsapp://"),
                "spotify" => Some("spotify://"),
                "discord" => Some("discord://"),
                "telegram" => Some("tg://"),
                "slack" => Some("slack://"),
                "teams" | "microsoft teams" => Some("msteams://"),
                "settings" | "windows settings" => Some("ms-settings://"),
                "store" | "microsoft store" => Some("ms-windows-store://"),
                "calculator" | "calc" => Some("calculator://"),
                "mail" | "outlook" => Some("outlookmail://"),
                "calendar" => Some("outlookcal://"),
                "maps" => Some("bingmaps://"),
                "photos" => Some("ms-photos://"),
                "camera" => Some("microsoft.windows.camera://"),
                "clock" | "alarms" => Some("ms-clock://"),
                _ => None,
            };

            if let Some(uri) = protocol {
                let output = tokio::process::Command::new("cmd")
                    .args(["/C", "start", "", uri])
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;
                if output.status.success() {
                    return Ok(format!("Opened: {}", app));
                }
            }

            // Try direct spawn (works for executables in PATH like notepad, code, etc.)
            {
                let mut cmd = tokio::process::Command::new(&app);
                for arg in &args {
                    cmd.arg(arg);
                }
                if cmd.spawn().is_ok() {
                    return Ok(format!("Opened: {}", app));
                }
            }

            // Try `start` command (resolves Start Menu shortcuts, registered apps)
            {
                let mut cmd_args = vec!["/C".to_string(), "start".to_string(), String::new(), app.clone()];
                cmd_args.extend(args.iter().cloned());
                let output = tokio::process::Command::new("cmd")
                    .args(&cmd_args)
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;
                if output.status.success() {
                    return Ok(format!("Opened: {}", app));
                }
            }

            // Last resort: search Start Menu for a matching .lnk shortcut
            let script = format!(
                r#"$q = '*{}*'; $paths = @("$env:APPDATA\Microsoft\Windows\Start Menu", "$env:ProgramData\Microsoft\Windows\Start Menu"); $found = $paths | ForEach-Object {{ Get-ChildItem -Path $_ -Filter "$q.lnk" -Recurse -ErrorAction SilentlyContinue }} | Select-Object -First 1; if ($found) {{ Start-Process $found.FullName; $found.Name }} else {{ Write-Error 'App not found' }}"#,
                app.replace('\'', "''")
            );
            match WindowsAdapter::powershell(&script).await {
                Ok(name) => Ok(format!("Opened: {}", name.trim_end_matches(".lnk"))),
                Err(_) => Err(format!("Could not find or open '{}'", app)),
            }
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
