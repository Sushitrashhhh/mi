"""App tools: open, list running, focus, close. Uses the OS adapter."""

from __future__ import annotations

import json

from mi_core.tools.os_adapt import get_adapter
from mi_core.tools.registry import Tier, ToolResult, tool


@tool(
    name="apps.open",
    tier=Tier.SAFE,
    description="Open an application by name, or open a URL in the default browser.",
    summary="Open {name}",
)
async def open_app(name: str) -> ToolResult:
    adapter = get_adapter()
    if name.startswith("http://") or name.startswith("https://"):
        msg = await adapter.open_url(name)
    else:
        msg = await adapter.open_app(name)
    return ToolResult(content=msg)


@tool(
    name="apps.list_running",
    tier=Tier.READ,
    description="List currently running applications.",
)
async def list_running() -> ToolResult:
    adapter = get_adapter()
    apps = await adapter.list_running()
    return ToolResult(content=json.dumps(apps[:30]))


@tool(
    name="apps.active_window",
    tier=Tier.READ,
    description="Get the title of the currently active window.",
)
async def active_window() -> ToolResult:
    adapter = get_adapter()
    title = await adapter.active_window()
    return ToolResult(content=title)


@tool(
    name="apps.focus",
    tier=Tier.SAFE,
    description="Bring a window to the foreground by partial title match.",
    summary="Focus window: {title}",
)
async def focus_window(title: str) -> ToolResult:
    adapter = get_adapter()
    msg = await adapter.focus_window(title)
    return ToolResult(content=msg)


@tool(
    name="apps.close",
    tier=Tier.CONFIRM,
    description="Close an application by process name.",
    summary="Close {name}",
)
async def close_app(name: str) -> ToolResult:
    adapter = get_adapter()
    msg = await adapter.close_app(name)
    return ToolResult(content=msg)
