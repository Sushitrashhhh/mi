"""LLM provider interface (BLUEPRINT 6.5). Phase 0 only needs text streaming + model listing."""

from __future__ import annotations

from collections.abc import AsyncIterator
from typing import Literal, Protocol

from pydantic import BaseModel


class Message(BaseModel):
    role: Literal["user", "assistant"]
    text: str


class Chunk(BaseModel):
    text: str = ""
    done: bool = False


class ModelInfo(BaseModel):
    id: str  # without the "models/" prefix
    display_name: str = ""
    actions: list[str] = []


class LLMProvider(Protocol):
    def stream(
        self, messages: list[Message], *, model: str, system: str | None = None
    ) -> AsyncIterator[Chunk]: ...

    async def list_models(self) -> list[ModelInfo]: ...


class FakeProvider:
    """Deterministic provider for tests: replays scripted replies, no network."""

    def __init__(self, replies: list[str] | None = None, models: list[ModelInfo] | None = None):
        self.replies = list(replies or ["hello from fake"])
        self.models = models or []
        self.calls: list[list[Message]] = []

    async def stream(
        self, messages: list[Message], *, model: str, system: str | None = None
    ) -> AsyncIterator[Chunk]:
        self.calls.append(messages)
        reply = self.replies.pop(0) if len(self.replies) > 1 else self.replies[0]
        for word in reply.split(" "):
            yield Chunk(text=word + " ")
        yield Chunk(done=True)

    async def list_models(self) -> list[ModelInfo]:
        return self.models
