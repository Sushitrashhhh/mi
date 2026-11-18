"""Tool registry: @tool decorator, Tier enum, schema generation for Gemini function calling."""

from __future__ import annotations

import enum
import inspect
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, get_type_hints


class Tier(enum.Enum):
    READ = "READ"
    SAFE = "SAFE"
    CONFIRM = "CONFIRM"
    BLOCKED = "BLOCKED"

    def __lt__(self, other: Tier) -> bool:
        return _TIER_ORDER[self] < _TIER_ORDER[other]

    def __le__(self, other: Tier) -> bool:
        return _TIER_ORDER[self] <= _TIER_ORDER[other]

    def escalated(self) -> Tier:
        order = list(Tier)
        idx = order.index(self)
        return order[min(idx + 1, len(order) - 1)]


_TIER_ORDER = {Tier.READ: 0, Tier.SAFE: 1, Tier.CONFIRM: 2, Tier.BLOCKED: 3}

_PYTHON_TO_JSON: dict[type, str] = {
    str: "string",
    int: "integer",
    float: "number",
    bool: "boolean",
}


@dataclass
class ToolResult:
    ok: bool = True
    content: str = ""
    artifacts: list[dict[str, Any]] | None = None
    undo: Callable[[], Any] | None = None


@dataclass
class ToolDef:
    name: str
    tier: Tier
    description: str
    summary: str
    fn: Callable[..., Any]
    parameters: dict[str, dict[str, Any]] = field(default_factory=dict)
    required: list[str] = field(default_factory=list)

    def render_summary(self, args: dict[str, Any]) -> str:
        try:
            return self.summary.format(**args)
        except (KeyError, IndexError):
            return f"{self.name}({', '.join(f'{k}={v!r}' for k, v in args.items())})"

    def to_function_declaration(self) -> dict[str, Any]:
        props: dict[str, Any] = {}
        for pname, pschema in self.parameters.items():
            props[pname] = pschema
        decl: dict[str, Any] = {
            "name": self.name.replace(".", "_"),
            "description": self.description,
        }
        if props:
            decl["parameters"] = {
                "type": "object",
                "properties": props,
                "required": self.required,
            }
        return decl


class ToolRegistry:
    def __init__(self) -> None:
        self._tools: dict[str, ToolDef] = {}

    def register(self, tool_def: ToolDef) -> None:
        self._tools[tool_def.name] = tool_def

    def get(self, name: str) -> ToolDef | None:
        return self._tools.get(name) or self._tools.get(name.replace("_", "."))

    def all_tools(self) -> list[ToolDef]:
        return list(self._tools.values())

    def function_declarations(self) -> list[dict[str, Any]]:
        return [t.to_function_declaration() for t in self._tools.values() if t.tier != Tier.BLOCKED]

    def lookup_by_gemini_name(self, gemini_name: str) -> ToolDef | None:
        for t in self._tools.values():
            if t.name.replace(".", "_") == gemini_name:
                return t
        return self.get(gemini_name)


_GLOBAL_REGISTRY = ToolRegistry()


def get_registry() -> ToolRegistry:
    return _GLOBAL_REGISTRY


def reset_registry() -> None:
    global _GLOBAL_REGISTRY
    _GLOBAL_REGISTRY = ToolRegistry()


def _extract_params(fn: Callable[..., Any]) -> tuple[dict[str, dict[str, Any]], list[str]]:
    hints = get_type_hints(fn)
    sig = inspect.signature(fn)
    params: dict[str, dict[str, Any]] = {}
    required: list[str] = []
    for pname, param in sig.parameters.items():
        if pname in ("self", "cls"):
            continue
        ptype = hints.get(pname, str)
        origin = getattr(ptype, "__origin__", None)
        if origin is list:
            json_type = "array"
        else:
            json_type = _PYTHON_TO_JSON.get(ptype, "string")
        schema: dict[str, Any] = {"type": json_type}
        # Pull description from docstring :param lines if available
        params[pname] = schema
        if param.default is inspect.Parameter.empty:
            required.append(pname)
    return params, required


def tool(
    *,
    name: str,
    tier: Tier,
    description: str,
    summary: str = "{name}",
) -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    def decorator(fn: Callable[..., Any]) -> Callable[..., Any]:
        params, req = _extract_params(fn)
        td = ToolDef(
            name=name,
            tier=tier,
            description=description,
            summary=summary,
            fn=fn,
            parameters=params,
            required=req,
        )
        _GLOBAL_REGISTRY.register(td)
        fn._tool_def = td  # type: ignore[attr-defined]
        return fn

    return decorator
