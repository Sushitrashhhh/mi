use std::io;
use std::process::Command;

pub fn install_autostart() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy();

    let output = Command::new("schtasks")
        .args([
            "/Create",
            "/SC", "ONLOGON",
            "/TN", "MiAssistant",
            "/TR", &format!("\"{}\" start", exe_str),
            "/RL", "LIMITED",
            "/F",
        ])
        .output()?;

    if output.status.success() {
        println!("Installed: mi will start at login.");
        println!("  Task name: MiAssistant");
        println!("  Command: \"{}\" start", exe_str);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!("schtasks failed: {}", stderr.trim())))
    }
}

pub fn uninstall_autostart() -> io::Result<()> {
    let output = Command::new("schtasks")
        .args(["/Delete", "/TN", "MiAssistant", "/F"])
        .output()?;

    if output.status.success() {
        println!("Uninstalled: mi will no longer start at login.");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!("schtasks failed: {}", stderr.trim())))
    }
}
