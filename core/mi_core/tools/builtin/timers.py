"""Timer tools: in-memory timers/reminders for Phase 1 (persistent scheduling in Phase 6)."""

from __future__ import annotations

import asyncio
import json
import time
from dataclasses import dataclass, field

from mi_core.tools.registry import Tier, ToolResult, tool

_timers: dict[str, TimerEntry] = {}


@dataclass
class TimerEntry:
    id: str
    label: str
    due_at: float
    task: asyncio.Task[None] | None = field(default=None, repr=False)


async def _timer_callback(timer_id: str, seconds: float) -> None:
    await asyncio.sleep(seconds)
    entry = _timers.get(timer_id)
    if entry:
        entry.task = None


@tool(
    name="timers.set",
    tier=Tier.SAFE,
    description="Set a timer/reminder. Duration in seconds. In-memory only (lost on restart).",
    summary="Timer: {label} in {seconds}s",
)
async def set_timer(label: str, seconds: int) -> ToolResult:
    timer_id = f"t_{int(time.time())}_{len(_timers)}"
    entry = TimerEntry(id=timer_id, label=label, due_at=time.time() + seconds)
    loop = asyncio.get_event_loop()
    entry.task = loop.create_task(_timer_callback(timer_id, seconds))
    _timers[timer_id] = entry
    return ToolResult(content=f"Timer set: '{label}' in {seconds}s (id: {timer_id})")


@tool(
    name="timers.list",
    tier=Tier.READ,
    description="List active timers.",
)
async def list_timers() -> ToolResult:
    now = time.time()
    active = []
    for entry in _timers.values():
        remaining = max(0, entry.due_at - now)
        active.append(
            {
                "id": entry.id,
                "label": entry.label,
                "remaining_seconds": int(remaining),
                "done": entry.task is None or entry.task.done(),
            }
        )
    return ToolResult(content=json.dumps(active))


@tool(
    name="timers.cancel",
    tier=Tier.SAFE,
    description="Cancel an active timer by ID.",
    summary="Cancel timer {timer_id}",
)
async def cancel_timer(timer_id: str) -> ToolResult:
    entry = _timers.pop(timer_id, None)
    if not entry:
        return ToolResult(ok=False, content=f"Timer not found: {timer_id}")
    if entry.task and not entry.task.done():
        entry.task.cancel()
    return ToolResult(content=f"Cancelled timer: {entry.label}")
