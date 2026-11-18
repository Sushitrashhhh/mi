"""FastAPI app: WebSocket event bus on 127.0.0.1, token required."""

from __future__ import annotations

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from pydantic import BaseModel, ValidationError

from mi_core import events as ev
from mi_core.agent.loop import AgentLoop, WebSocketApprovalHandler
from mi_core.config import MiConfig
from mi_core.llm.base import LLMProvider, Message
from mi_core.server.auth import token_ok
from mi_core.tools.registry import ToolRegistry


def create_app(
    config: MiConfig, provider: LLMProvider, token: str, registry: ToolRegistry | None = None
) -> FastAPI:
    app = FastAPI(title="mi", docs_url=None, redoc_url=None, openapi_url=None)

    @app.get("/health")
    async def health() -> dict[str, str]:
        return {"status": "ok"}

    @app.websocket("/ws")
    async def ws_endpoint(ws: WebSocket) -> None:
        if not token_ok(ws.query_params.get("token"), token):
            await ws.close(code=4401)
            return
        await ws.accept()
        history: list[Message] = []
        approval_handler = WebSocketApprovalHandler()
        reg = registry or _default_registry()
        agent = AgentLoop(config, provider, reg, approval_handler)
        try:
            while True:
                raw = await ws.receive_text()
                try:
                    env = ev.Envelope.model_validate_json(raw)
                    payload = env.payload()
                except (ValidationError, ValueError) as e:
                    await _send(ws, "error", ev.Notice(level="error", message=f"bad event: {e}"))
                    continue
                if isinstance(payload, ev.UserMessage):
                    await _handle_user_message(ws, env, payload, agent, history)
                elif isinstance(payload, ev.ApprovalResponse):
                    approval_handler.resolve(payload)
        except WebSocketDisconnect:
            return

    return app


def _default_registry() -> ToolRegistry:
    from mi_core.tools.builtin import register_all
    from mi_core.tools.registry import get_registry

    register_all()
    return get_registry()


async def _send(ws: WebSocket, type_: str, payload: BaseModel, session: str = "default") -> None:
    await ws.send_text(ev.make_event(type_, payload, session).model_dump_json())


async def _handle_user_message(
    ws: WebSocket,
    env: ev.Envelope,
    msg: ev.UserMessage,
    agent: AgentLoop,
    history: list[Message],
) -> None:
    async for event in agent.run(msg.text, history, env.session):
        if isinstance(event, ev.AssistantDelta):
            await _send(ws, "assistant.delta", event, env.session)
        elif isinstance(event, ev.AssistantDone):
            await _send(ws, "assistant.done", event, env.session)
        elif isinstance(event, ev.ToolStarted):
            await _send(ws, "tool.started", event, env.session)
        elif isinstance(event, ev.ToolFinished):
            await _send(ws, "tool.finished", event, env.session)
        elif isinstance(event, ev.ApprovalRequest):
            await _send(ws, "approval.request", event, env.session)
        elif isinstance(event, ev.Notice):
            await _send(ws, "notice", event, env.session)
