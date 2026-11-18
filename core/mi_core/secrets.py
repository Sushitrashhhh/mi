"""Secrets live in the OS keyring. GEMINI_API_KEY env var is a dev-only fallback."""

from __future__ import annotations

import os

import keyring

SERVICE = "mi"
GEMINI_KEY = "gemini-api-key"


def get_gemini_key() -> str | None:
    env = os.environ.get("GEMINI_API_KEY")
    if env:
        return env
    try:
        return keyring.get_password(SERVICE, GEMINI_KEY)
    except Exception:
        return None


def set_gemini_key(key: str) -> None:
    keyring.set_password(SERVICE, GEMINI_KEY, key.strip())


def delete_gemini_key() -> None:
    try:
        keyring.delete_password(SERVICE, GEMINI_KEY)
    except Exception:
        pass
