"""System tools: time, battery, CPU, volume, notifications."""

from __future__ import annotations

import datetime
import json
import platform

from mi_core.tools.os_adapt import get_adapter
from mi_core.tools.registry import Tier, ToolResult, tool


@tool(name="system.time", tier=Tier.READ, description="Get current date and time.")
async def get_time() -> ToolResult:
    now = datetime.datetime.now()
    return ToolResult(content=now.strftime("%Y-%m-%d %H:%M:%S %A"))


@tool(name="system.info", tier=Tier.READ, description="Get system info (OS, platform).")
async def system_info() -> ToolResult:
    info = {
        "os": platform.system(),
        "release": platform.release(),
        "version": platform.version(),
        "machine": platform.machine(),
        "processor": platform.processor(),
    }
    return ToolResult(content=json.dumps(info))


@tool(
    name="system.battery",
    tier=Tier.READ,
    description="Get battery level and charging status.",
)
async def battery() -> ToolResult:
    adapter = get_adapter()
    data = await adapter.get_battery()
    return ToolResult(content=json.dumps(data))


@tool(
    name="system.volume",
    tier=Tier.SAFE,
    description="Set system volume to a percentage (0-100).",
    summary="Set volume to {level}%",
)
async def set_volume(level: int) -> ToolResult:
    adapter = get_adapter()
    msg = await adapter.set_volume(level)
    return ToolResult(content=msg)


@tool(
    name="system.notify",
    tier=Tier.SAFE,
    description="Show a desktop notification.",
    summary="Notify: {title}",
)
async def notify(title: str, body: str) -> ToolResult:
    adapter = get_adapter()
    msg = await adapter.notify(title, body)
    return ToolResult(content=msg)
