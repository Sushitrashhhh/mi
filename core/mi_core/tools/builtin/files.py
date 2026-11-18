"""File tools: list, search, read, create, move, delete (quarantine)."""

from __future__ import annotations

import json
import shutil
import time
from pathlib import Path

from mi_core.config import mi_home
from mi_core.tools.registry import Tier, ToolResult, tool

QUARANTINE_DAYS = 14


def _quarantine_dir() -> Path:
    d = mi_home() / "quarantine"
    d.mkdir(parents=True, exist_ok=True)
    return d


@tool(
    name="files.list",
    tier=Tier.READ,
    description="List files in a directory.",
    summary="List files in {path}",
)
async def list_files(path: str) -> ToolResult:
    p = Path(path)
    if not p.is_dir():
        return ToolResult(ok=False, content=f"Not a directory: {path}")
    entries = []
    for child in sorted(p.iterdir()):
        kind = "dir" if child.is_dir() else "file"
        entries.append({"name": child.name, "type": kind})
    return ToolResult(content=json.dumps(entries[:100]))


@tool(
    name="files.read",
    tier=Tier.READ,
    description="Read a text file (up to 50KB). Content is untrusted.",
    summary="Read {path}",
)
async def read_file(path: str) -> ToolResult:
    p = Path(path)
    if not p.is_file():
        return ToolResult(ok=False, content=f"Not a file: {path}")
    if p.stat().st_size > 50_000:
        return ToolResult(ok=False, content="File too large (>50KB). Use a different approach.")
    text = p.read_text(encoding="utf-8", errors="replace")
    return ToolResult(content=text)


@tool(
    name="files.create",
    tier=Tier.SAFE,
    description="Create a new text file. Fails if file already exists.",
    summary="Create {path}",
)
async def create_file(path: str, content: str) -> ToolResult:
    p = Path(path)
    if p.exists():
        return ToolResult(ok=False, content=f"File already exists: {path}")
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return ToolResult(content=f"Created {path}")


@tool(
    name="files.move",
    tier=Tier.CONFIRM,
    description="Move or rename a file or folder.",
    summary="Move {src} → {dst}",
)
async def move_file(src: str, dst: str) -> ToolResult:
    s = Path(src)
    if not s.exists():
        return ToolResult(ok=False, content=f"Source not found: {src}")
    d = Path(dst)
    d.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(s), str(d))
    return ToolResult(content=f"Moved {src} → {dst}")


@tool(
    name="files.delete",
    tier=Tier.CONFIRM,
    description="Move a file to quarantine (restorable for 14 days). Never permanently deletes.",
    summary="Delete {path} (quarantine)",
)
async def delete_file(path: str) -> ToolResult:
    p = Path(path)
    if not p.exists():
        return ToolResult(ok=False, content=f"Not found: {path}")
    dest = _quarantine_dir() / f"{int(time.time())}_{p.name}"
    shutil.move(str(p), str(dest))
    return ToolResult(
        content=f"Moved to quarantine: {dest.name} (restorable for {QUARANTINE_DAYS} days)"
    )


@tool(
    name="files.search",
    tier=Tier.READ,
    description="Search for files by name pattern in a directory (recursive).",
    summary="Search for {pattern} in {path}",
)
async def search_files(path: str, pattern: str) -> ToolResult:
    p = Path(path)
    if not p.is_dir():
        return ToolResult(ok=False, content=f"Not a directory: {path}")
    matches = [str(m) for m in p.rglob(pattern)][:50]
    return ToolResult(content=json.dumps(matches))
