"""Gemini provider built on the official google-genai SDK."""

from __future__ import annotations

from collections.abc import AsyncIterator
from typing import Any

from google import genai
from google.genai import types

from mi_core.llm.base import Chunk, FunctionCall, Message, ModelInfo


class GeminiError(Exception):
    pass


def _message_to_content(m: Message) -> types.Content:
    if m.role == "tool" and m.function_responses:
        resp_parts = [
            types.Part(
                function_response=types.FunctionResponse(
                    name=fr.name,
                    response={"result": fr.result},
                )
            )
            for fr in m.function_responses
        ]
        return types.Content(role="user", parts=resp_parts)
    if m.role == "assistant" and m.function_calls:
        parts: list[types.Part] = []
        if m.text:
            parts.append(types.Part(text=m.text))
        for fc in m.function_calls:
            part_kwargs: dict[str, Any] = {
                "function_call": types.FunctionCall(name=fc.name, args=fc.args),
            }
            if fc.thought_signature:
                part_kwargs["thought_signature"] = fc.thought_signature
            parts.append(types.Part(**part_kwargs))
        return types.Content(role="model", parts=parts)
    role = "model" if m.role == "assistant" else "user"
    return types.Content(role=role, parts=[types.Part(text=m.text)])


class GeminiProvider:
    def __init__(self, api_key: str, client: Any | None = None):
        self._client = client or genai.Client(api_key=api_key)

    async def stream(
        self,
        messages: list[Message],
        *,
        model: str,
        system: str | None = None,
        tools: list[dict[str, Any]] | None = None,
    ) -> AsyncIterator[Chunk]:
        if not model:
            raise GeminiError(
                "No model configured. Run `mi doctor --fix` (or edit [models] in ~/.mi/mi.toml)."
            )
        contents = [_message_to_content(m) for m in messages]
        config_kwargs: dict[str, Any] = {}
        if system:
            config_kwargs["system_instruction"] = system
        if tools:
            gemini_tools = [
                types.Tool(
                    function_declarations=[types.FunctionDeclaration(**decl) for decl in tools]
                )
            ]
            config_kwargs["tools"] = gemini_tools
        config = types.GenerateContentConfig(**config_kwargs) if config_kwargs else None
        try:
            stream = await self._client.aio.models.generate_content_stream(
                model=model, contents=contents, config=config
            )
            async for part in stream:
                fc_list: list[FunctionCall] = []
                if part.candidates:
                    for candidate in part.candidates:
                        if candidate.content and candidate.content.parts:
                            for p in candidate.content.parts:
                                if p.function_call:
                                    sig = getattr(p, "thought_signature", None)
                                    fc_list.append(
                                        FunctionCall(
                                            name=p.function_call.name or "",
                                            args=dict(p.function_call.args or {}),
                                            call_id=p.function_call.name or "",
                                            thought_signature=sig,
                                        )
                                    )
                if fc_list:
                    yield Chunk(function_calls=fc_list)
                elif part.text:
                    yield Chunk(text=part.text)
        except Exception as e:
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
