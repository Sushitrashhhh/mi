"""Gemini provider built on the official google-genai SDK."""

from __future__ import annotations

from collections.abc import AsyncIterator
from typing import Any

from google import genai
from google.genai import types

from mi_core.llm.base import Chunk, Message, ModelInfo


class GeminiError(Exception):
    pass


class GeminiProvider:
    def __init__(self, api_key: str, client: Any | None = None):
        self._client = client or genai.Client(api_key=api_key)

    async def stream(
        self, messages: list[Message], *, model: str, system: str | None = None
    ) -> AsyncIterator[Chunk]:
        if not model:
            raise GeminiError(
                "No model configured. Run `mi doctor --fix` (or edit [models] in ~/.mi/mi.toml)."
            )
        contents = [
            types.Content(
                role="model" if m.role == "assistant" else "user",
                parts=[types.Part(text=m.text)],
            )
            for m in messages
        ]
        config = types.GenerateContentConfig(system_instruction=system) if system else None
        try:
            stream = await self._client.aio.models.generate_content_stream(
                model=model, contents=contents, config=config
            )
            async for part in stream:
                if part.text:
                    yield Chunk(text=part.text)
        except Exception as e:  # SDK raises various errors; surface a clean one
            raise GeminiError(str(e)) from e
        yield Chunk(done=True)

    async def list_models(self) -> list[ModelInfo]:
        out: list[ModelInfo] = []
        try:
            pager = await self._client.aio.models.list()
            async for m in pager:
                name = (m.name or "").removeprefix("models/")
                out.append(
                    ModelInfo(
                        id=name,
                        display_name=m.display_name or "",
                        actions=list(m.supported_actions or []),
                    )
                )
        except Exception as e:
            raise GeminiError(str(e)) from e
        return sorted(out, key=lambda m: m.id)
