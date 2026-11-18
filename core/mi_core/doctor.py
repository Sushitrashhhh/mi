"""`mi doctor`: validate key + configured model IDs against client.models.list()."""

from __future__ import annotations

import difflib
from dataclasses import dataclass, field

from mi_core.config import ModelsConfig
from mi_core.llm.base import ModelInfo

CLOUD_ROLES = ["fast", "default", "deep", "vision", "live", "tts", "embed"]


@dataclass
class RoleCheck:
    role: str
    configured: str
    status: str  # "ok" | "unset" | "invalid"
    suggestions: list[str] = field(default_factory=list)


def check_models(models_cfg: ModelsConfig, available: list[ModelInfo]) -> list[RoleCheck]:
    ids = [m.id for m in available]
    out: list[RoleCheck] = []
    for role in CLOUD_ROLES:
        configured = getattr(models_cfg, role)
        if not configured:
            out.append(RoleCheck(role, "", "unset", suggest_for_role(role, available)))
        elif configured in ids:
            out.append(RoleCheck(role, configured, "ok"))
        else:
            near = difflib.get_close_matches(configured, ids, n=3, cutoff=0.5)
            out.append(
                RoleCheck(role, configured, "invalid", near or suggest_for_role(role, available))
            )
    return out


def _supports(m: ModelInfo, action: str) -> bool:
    return action in m.actions


def suggest_for_role(role: str, available: list[ModelInfo]) -> list[str]:
    """Pick candidates *from the live list* (never from a hardcoded ID).

    Heuristic on the name family only; the user can always override in mi.toml.
    """
    gen = [m for m in available if _supports(m, "generateContent")]
    stable = [m for m in gen if "preview" not in m.id and "exp" not in m.id]

    def newest(models: list[ModelInfo]) -> list[str]:
        return [m.id for m in sorted(models, key=lambda m: m.id, reverse=True)][:3]

    if role == "fast":
        return newest([m for m in stable if "flash-lite" in m.id]) or newest(
            [m for m in gen if "flash-lite" in m.id]
        )
    if role in ("default", "vision"):
        return newest([m for m in stable if "flash" in m.id and "lite" not in m.id])
    if role == "deep":
        return newest([m for m in stable if "pro" in m.id])
    if role == "embed":
        return newest([m for m in available if _supports(m, "embedContent")])
    if role == "tts":
        return newest([m for m in available if "tts" in m.id])
    if role == "live":
        return newest(
            [m for m in available if _supports(m, "bidiGenerateContent") or "live" in m.id]
        )
    return []
