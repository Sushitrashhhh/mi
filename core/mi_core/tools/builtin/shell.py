"""Shell tool: run a command. Always CONFIRM unless allowlisted in policy.toml."""

from __future__ import annotations

from mi_core.tools.os_adapt import get_adapter
from mi_core.tools.registry import Tier, ToolResult, tool


@tool(
    name="shell.run",
    tier=Tier.CONFIRM,
    description="Run a shell command and return its output. Always requires approval.",
    summary="Run: {cmd}",
)
async def run_command(cmd: str, timeout: int = 30) -> ToolResult:
    adapter = get_adapter()
    output, code = await adapter.run_command(cmd, timeout=timeout)
    return ToolResult(ok=code == 0, content=output)
