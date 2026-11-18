"""Event protocol: single source of truth (BLUEPRINT 5.1). Every message is an Envelope."""

from __future__ import annotations

import uuid
from datetime import UTC, datetime
from typing import Any, Literal

from pydantic import BaseModel, Field


class Attachment(BaseModel):
    kind: Literal["image", "file"]
    path: str | None = None
    b64: str | None = None


# ---- client -> core ----
class UserMessage(BaseModel):
    text: str
    attachments: list[Attachment] = Field(default_factory=list)
    channel: Literal["overlay", "cli", "voice", "telegram"] = "cli"


class UserCancel(BaseModel):
    turn_id: str


class ApprovalResponse(BaseModel):
    approval_id: str
    decision: Literal["approve", "deny", "approve_always"]
    note: str | None = None


class VoiceControl(BaseModel):
    action: Literal["ptt_start", "ptt_stop", "mute", "unmute"]


# ---- core -> client ----
class Citation(BaseModel):
    title: str
    url: str | None = None
    path: str | None = None


class AssistantDelta(BaseModel):
    turn_id: str
    text: str


class AssistantDone(BaseModel):
    turn_id: str
    text: str
    citations: list[Citation] = Field(default_factory=list)


class ToolStarted(BaseModel):
    turn_id: str
    call_id: str
    tool: str
    args_preview: str


class ToolFinished(BaseModel):
    turn_id: str
    call_id: str
    tool: str
    args_preview: str
    result_preview: str | None = None
    ok: bool = True


class ApprovalRequest(BaseModel):
    approval_id: str
    tool: str
    tier: Literal["READ", "SAFE", "CONFIRM", "BLOCKED"]
    summary: str
    args: dict[str, Any] = Field(default_factory=dict)
    reason: str = ""
    expires_at: str


class VoiceState(BaseModel):
    state: Literal["idle", "listening", "thinking", "speaking"]


class MemorySaved(BaseModel):
    memory_id: str
    kind: str
    text: str


class Notice(BaseModel):
    level: Literal["info", "warning", "error"] = "info"
    message: str


# type string -> payload model
EVENT_TYPES: dict[str, type[BaseModel]] = {
    "user.message": UserMessage,
    "user.cancel": UserCancel,
    "approval.response": ApprovalResponse,
    "voice.control": VoiceControl,
    "assistant.delta": AssistantDelta,
    "assistant.done": AssistantDone,
    "tool.started": ToolStarted,
    "tool.finished": ToolFinished,
    "approval.request": ApprovalRequest,
    "voice.state": VoiceState,
    "memory.saved": MemorySaved,
    "notice": Notice,
    "error": Notice,
}


def _now() -> str:
    return datetime.now(UTC).isoformat()


class Envelope(BaseModel):
    type: str
    id: str = Field(default_factory=lambda: str(uuid.uuid4()))
    ts: str = Field(default_factory=_now)
    session: str = "default"
    data: dict[str, Any] = Field(default_factory=dict)

    def payload(self) -> BaseModel:
        """Validate `data` against the model registered for `type`."""
        model = EVENT_TYPES.get(self.type)
        if model is None:
            raise ValueError(f"unknown event type: {self.type}")
        return model.model_validate(self.data)


def make_event(type_: str, payload: BaseModel, session: str = "default") -> Envelope:
    if type_ not in EVENT_TYPES:
        raise ValueError(f"unknown event type: {type_}")
    return Envelope(type=type_, session=session, data=payload.model_dump())
