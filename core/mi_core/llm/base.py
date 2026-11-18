"""LLM provider interface (BLUEPRINT 6.5). Supports text streaming + function calling."""

from __future__ import annotations

from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from typing import Any, Literal, Protocol

from pydantic import BaseModel, Field


class FunctionCall(BaseModel):
    name: str
    args: dict[str, Any] = Field(default_factory=dict)
    call_id: str = ""
    thought_signature: bytes | None = None


class FunctionResponse(BaseModel):
    call_id: str
    name: str
    result: str


class Message(BaseModel):
    role: Literal["user", "assistant", "tool"]
    text: str = ""
    function_calls: list[FunctionCall] = Field(default_factory=list)
    function_responses: list[FunctionResponse] = Field(default_factory=list)


class Chunk(BaseModel):
    text: str = ""
    function_calls: list[FunctionCall] = Field(default_factory=list)
    done: bool = False


class ModelInfo(BaseModel):
    id: str
    display_name: str = ""
    actions: list[str] = []


class LLMProvider(Protocol):
    def stream(
        self,
        messages: list[Message],
        *,
        model: str,
        system: str | None = None,
        tools: list[dict[str, Any]] | None = None,
    ) -> AsyncIterator[Chunk]: ...

    async def list_models(self) -> list[ModelInfo]: ...


@dataclass
class ScriptedReply:
    text: str = ""
    function_calls: list[FunctionCall] = field(default_factory=list)


class FakeProvider:
    """Deterministic provider for tests: replays scripted replies, no network."""

    def __init__(
        self,
        replies: list[str | ScriptedReply] | None = None,
        models: list[ModelInfo] | None = None,
    ):
        self.replies: list[ScriptedReply] = []
        for r in replies or ["hello from fake"]:
            if isinstance(r, str):
                self.replies.append(ScriptedReply(text=r))
            else:
                self.replies.append(r)
        self.models = models or []
        self.calls: list[list[Message]] = []

    async def stream(
        self,
        messages: list[Message],
        *,
        model: str,
        system: str | None = None,
        tools: list[dict[str, Any]] | None = None,
    ) -> AsyncIterator[Chunk]:
        self.calls.append(messages)
        reply = self.replies.pop(0) if len(self.replies) > 1 else self.replies[0]
        if reply.function_calls:
            yield Chunk(function_calls=reply.function_calls)
        if reply.text:
            for word in reply.text.split(" "):
                yield Chunk(text=word + " ")
        yield Chunk(done=True)

    async def list_models(self) -> list[ModelInfo]:
        return self.models
