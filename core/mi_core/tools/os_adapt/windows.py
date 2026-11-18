"""Windows OS adapter: subprocess-based, no pywinauto dependency in Phase 1."""

from __future__ import annotations

import asyncio
import os
import subprocess


class WindowsAdapter:
    async def open_app(self, name: str) -> str:
        await asyncio.to_thread(os.startfile, name)
        return f"Opened {name}"

    async def open_url(self, url: str) -> str:
        await asyncio.to_thread(os.startfile, url)
        return f"Opened {url}"

    async def list_running(self) -> list[str]:
        result = await asyncio.to_thread(
            subprocess.run,
            ["tasklist", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        names = set()
        for line in result.stdout.strip().splitlines():
            parts = line.split('","')
            if parts:
                names.add(parts[0].strip('"'))
        return sorted(names)

    async def active_window(self) -> str:
        result = await asyncio.to_thread(
            subprocess.run,
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "(Get-Process | Where-Object {$_.MainWindowTitle} "
                "| Select-Object -First 1).MainWindowTitle",
            ],
            capture_output=True,
            text=True,
            timeout=5,
        )
        return result.stdout.strip() or "unknown"

    async def focus_window(self, title: str) -> str:
        script = (
            f'$w = Get-Process | Where-Object {{$_.MainWindowTitle -like "*{title}*"}} '
            f"| Select-Object -First 1; "
            "if($w) { [void][System.Reflection.Assembly]"
            '::LoadWithPartialName("Microsoft.VisualBasic"); '
            f"[Microsoft.VisualBasic.Interaction]::AppActivate($w.Id) }}"
        )
        await asyncio.to_thread(
            subprocess.run,
            ["powershell", "-NoProfile", "-Command", script],
            capture_output=True,
            timeout=5,
        )
        return f"Focused window matching '{title}'"

    async def close_app(self, name: str) -> str:
        await asyncio.to_thread(
            subprocess.run,
            ["taskkill", "/IM", name, "/F"],
            capture_output=True,
            timeout=10,
        )
        return f"Closed {name}"

    async def send_keys(self, keys: str) -> str:
        script = (
            '[void][System.Reflection.Assembly]::LoadWithPartialName("System.Windows.Forms"); '
            f'[System.Windows.Forms.SendKeys]::SendWait("{keys}")'
        )
        await asyncio.to_thread(
            subprocess.run,
            ["powershell", "-NoProfile", "-Command", script],
            capture_output=True,
            timeout=5,
        )
        return f"Sent keys: {keys}"

    async def notify(self, title: str, body: str) -> str:
        script = (
            "[void][System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms');"
            "$n = New-Object System.Windows.Forms.NotifyIcon;"
            "$n.Icon = [System.Drawing.SystemIcons]::Information;"
            "$n.Visible = $true;"
            f"$n.ShowBalloonTip(5000, '{title}', '{body}', 'Info');"
            "Start-Sleep -Seconds 6; $n.Dispose()"
        )
        await asyncio.to_thread(
            subprocess.run,
            ["powershell", "-NoProfile", "-Command", script],
            capture_output=True,
            timeout=15,
        )
        return f"Notification shown: {title}"

    async def set_volume(self, level: int) -> str:
        script = (
            "$vol = New-Object -ComObject WScript.Shell; "
            + "".join(["$vol.SendKeys([char]174); " for _ in range(50)])
            + "".join(["$vol.SendKeys([char]175); " for _ in range(level // 2)])
        )
        await asyncio.to_thread(
            subprocess.run,
            ["powershell", "-NoProfile", "-Command", script],
            capture_output=True,
            timeout=10,
        )
        return f"Volume set to ~{level}%"

    async def get_battery(self) -> dict[str, object]:
        result = await asyncio.to_thread(
            subprocess.run,
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Battery | "
                "Select-Object EstimatedChargeRemaining, BatteryStatus "
                "| ConvertTo-Json)",
            ],
            capture_output=True,
            text=True,
            timeout=5,
        )
        import json

        try:
            data = json.loads(result.stdout)
            return {
                "percent": data.get("EstimatedChargeRemaining", -1),
                "charging": data.get("BatteryStatus", 0) == 2,
            }
        except (json.JSONDecodeError, TypeError):
            return {"percent": -1, "charging": False}

    async def run_command(self, cmd: str, timeout: int = 30) -> tuple[str, int]:
        proc = await asyncio.create_subprocess_shell(
            cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
            limit=1024 * 1024,
        )
        try:
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=timeout)
        except TimeoutError:
            proc.kill()
            return "Command timed out.", 1
        output = stdout.decode("utf-8", errors="replace")[:10000]
        return output, proc.returncode or 0
