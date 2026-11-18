"""Audit log: append-only JSONL at ~/.mi/logs/audit.jsonl."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from mi_core.config import mi_home


def _log_path() -> Path:
    p = mi_home() / "logs"
    p.mkdir(parents=True, exist_ok=True)
    return p / "audit.jsonl"


def log_event(kind: str, **data: Any) -> None:
    entry = {"ts": datetime.now(UTC).isoformat(), "kind": kind, **data}
    with _log_path().open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry, default=str) + "\n")


def read_log(tail: int = 50) -> list[dict[str, Any]]:
    path = _log_path()
    if not path.exists():
        return []
    lines = path.read_text(encoding="utf-8").strip().splitlines()
    return [json.loads(line) for line in lines[-tail:]]
