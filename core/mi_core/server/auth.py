"""Local auth token: random, written to ~/.mi/token, required on every connection."""

from __future__ import annotations

import hmac
import os
import secrets as pysecrets
from pathlib import Path

from mi_core.config import ensure_home, mi_home


def token_path() -> Path:
    return mi_home() / "token"


def load_or_create_token() -> str:
    ensure_home()
    p = token_path()
    if p.exists():
        t = p.read_text(encoding="utf-8").strip()
        if t:
            return t
    t = pysecrets.token_urlsafe(32)
    p.write_text(t, encoding="utf-8")
    try:
        os.chmod(p, 0o600)
    except OSError:
        pass
    return t


def read_token() -> str | None:
    p = token_path()
    return p.read_text(encoding="utf-8").strip() if p.exists() else None


def token_ok(given: str | None, expected: str) -> bool:
    return bool(given) and hmac.compare_digest(given or "", expected)
