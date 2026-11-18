"""Config loading: ~/.mi/mi.toml (+ MI_HOME override), validated with pydantic."""

from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field


def mi_home() -> Path:
    """Runtime data dir. Override with MI_HOME (used by tests)."""
    return Path(os.environ.get("MI_HOME") or Path.home() / ".mi")


class CoreConfig(BaseModel):
    host: Literal["127.0.0.1"] = "127.0.0.1"  # never anything but localhost
    port: int = 7777
    autonomy: Literal["normal", "dry_run", "paused"] = "normal"
    max_steps: int = 12


class PrivacyConfig(BaseModel):
    mode: Literal["cloud", "hybrid", "local"] = "hybrid"
    keep_screenshots: bool = False
    redact_screenshots: bool = True
    retention_days: int = 90


class ModelsConfig(BaseModel):
    fast: str = ""
    default: str = ""
    deep: str = ""
    vision: str = ""
    live: str = ""
    tts: str = ""
    embed: str = ""
    local: str = "llama3.2:3b"


class VoiceConfig(BaseModel):
    pipeline: Literal["live", "pipeline"] = "live"
    wake_word: str = "hey_mi"
    wake_threshold: float = 0.6
    ptt_hotkey: str = "ctrl+space"
    silence_timeout_s: int = 8
    tts_voice: str = ""
    barge_in: bool = True


class MemoryConfig(BaseModel):
    top_k: int = 6
    auto_save: Literal["off", "stated_only", "all"] = "stated_only"


class MiConfig(BaseModel):
    core: CoreConfig = Field(default_factory=CoreConfig)
    privacy: PrivacyConfig = Field(default_factory=PrivacyConfig)
    models: ModelsConfig = Field(default_factory=ModelsConfig)
    voice: VoiceConfig = Field(default_factory=VoiceConfig)
    memory: MemoryConfig = Field(default_factory=MemoryConfig)


# Embedded default, so the single binary needs no data files next to it.
DEFAULT_TOML = """\
[core]
host = "127.0.0.1"
port = 7777
autonomy = "normal"
max_steps = 12

[privacy]
mode = "hybrid"
keep_screenshots = false
redact_screenshots = true
retention_days = 90

# Model IDs are NOT hardcoded. Run `mi doctor` to list the valid ones, then fill these in
# (or run `mi doctor --fix` to pick defaults from the list your key can use).
[models]
fast = ""
default = ""
deep = ""
vision = ""
live = ""
tts = ""
embed = ""
local = "llama3.2:3b"

[voice]
pipeline = "live"
wake_word = "hey_mi"
wake_threshold = 0.6
ptt_hotkey = "ctrl+space"
silence_timeout_s = 8
tts_voice = ""
barge_in = true

[memory]
top_k = 6
auto_save = "stated_only"
"""


def config_path() -> Path:
    return mi_home() / "mi.toml"


def ensure_home() -> Path:
    """Create ~/.mi and a default mi.toml on first run."""
    home = mi_home()
    home.mkdir(parents=True, exist_ok=True)
    if not config_path().exists():
        config_path().write_text(DEFAULT_TOML, encoding="utf-8")
    return home


def load_config() -> MiConfig:
    ensure_home()
    with config_path().open("rb") as f:
        raw = tomllib.load(f)
    return MiConfig.model_validate(raw)


def set_model(role: str, model_id: str) -> None:
    """Update one [models] entry in mi.toml (line rewrite, keeps comments)."""
    if role not in ModelsConfig.model_fields:
        raise ValueError(f"unknown model role: {role}")
    ensure_home()
    lines = config_path().read_text(encoding="utf-8").splitlines()
    in_models = False
    for i, line in enumerate(lines):
        s = line.strip()
        if s.startswith("["):
            in_models = s == "[models]"
        elif in_models and (s.startswith(f"{role} ") or s.startswith(f"{role}=")):
            lines[i] = f'{role} = "{model_id}"'
    config_path().write_text("\n".join(lines) + "\n", encoding="utf-8")
