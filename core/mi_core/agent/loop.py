"""Agent loop: streams LLM, handles function calls, enforces policy, emits events."""

from __future__ import annotations

import asyncio
import json
import uuid
from collections.abc import AsyncIterator
from pathlib import Path
from typing import Any

from mi_core.agent import audit
from mi_core.config import MiConfig
from mi_core.events import (
    ApprovalRequest,
    ApprovalResponse,
    AssistantDelta,
    AssistantDone,
    Notice,
    ToolFinished,
    ToolStarted,
)
from mi_core.llm.base import FunctionCall, FunctionResponse, LLMProvider, Message
from mi_core.permissions.policy import Decision, check_policy
from mi_core.tools.registry import Tier, ToolDef, ToolRegistry, ToolResult


def _load_system_prompt() -> str:
    prompt_file = Path(__file__).parent / "prompts" / "system.md"
    return prompt_file.read_text(encoding="utf-8")


SYSTEM_PROMPT = _load_system_prompt()


class TurnContext:
    def __init__(self, turn_id: str, session: str = "default"):
        self.turn_id = turn_id
        self.session = session
        self.tainted = False
        self.cancelled = False


class AgentLoop:
    def __init__(
        self,
        config: MiConfig,
        provider: LLMProvider,
        registry: ToolRegistry,
        approval_handler: ApprovalHandler | None = None,
    ):
        self.config = config
        self.provider = provider
        self.registry = registry
        self.approval_handler = approval_handler or _DenyAllHandler()

    async def run(
        self,
        user_text: str,
        history: list[Message],
        session: str = "default",
    ) -> AsyncIterator[Any]:
        turn_id = str(uuid.uuid4())
        ctx = TurnContext(turn_id, session)

        history.append(Message(role="user", text=user_text))

        tool_decls = self.registry.function_declarations()
        model = self.config.models.default
        max_steps = self.config.core.max_steps

        audit.log_event("turn_start", turn_id=turn_id, user_text=user_text[:200])

        for _step in range(max_steps):
            if ctx.cancelled:
                yield Notice(level="info", message="Turn cancelled.")
                break

            full_text = ""
            pending_calls: list[FunctionCall] = []

            async for chunk in self.provider.stream(
                history,
                model=model,
                system=SYSTEM_PROMPT,
                tools=tool_decls if tool_decls else None,
            ):
                if chunk.function_calls:
                    pending_calls.extend(chunk.function_calls)
                if chunk.text:
                    full_text += chunk.text
                    yield AssistantDelta(turn_id=turn_id, text=chunk.text)

            if not pending_calls:
                history.append(Message(role="assistant", text=full_text))
                audit.log_event(
                    "turn_done",
                    turn_id=turn_id,
                    model=model,
                    text_len=len(full_text),
                )
                yield AssistantDone(turn_id=turn_id, text=full_text)
                return

            history.append(Message(role="assistant", text=full_text, function_calls=pending_calls))

            responses: list[FunctionResponse] = []
            for fc in pending_calls:
                call_id = fc.call_id or str(uuid.uuid4())
                tool_def = self.registry.lookup_by_gemini_name(fc.name)
                if not tool_def:
                    responses.append(
                        FunctionResponse(
                            call_id=call_id,
                            name=fc.name,
                            result=json.dumps({"error": f"Unknown tool: {fc.name}"}),
                        )
                    )
                    continue

                yield ToolStarted(
                    turn_id=turn_id,
                    call_id=call_id,
                    tool=tool_def.name,
                    args_preview=_preview_args(fc.args),
                )

                result = await self._execute_tool(ctx, tool_def, fc.args, call_id)

                audit.log_event(
                    "tool_call",
                    turn_id=turn_id,
                    call_id=call_id,
                    tool=tool_def.name,
                    tier=tool_def.tier.value,
                    args=fc.args,
                    ok=result.ok,
                )

                if tool_def.tier == Tier.READ and not result.ok:
                    pass
                if _content_may_taint(tool_def):
                    ctx.tainted = True

                yield ToolFinished(
                    turn_id=turn_id,
                    call_id=call_id,
                    tool=tool_def.name,
                    args_preview=_preview_args(fc.args),
                    result_preview=result.content[:200] if result.content else None,
                    ok=result.ok,
                )

                responses.append(
                    FunctionResponse(
                        call_id=call_id,
                        name=fc.name,
                        result=result.content or json.dumps({"ok": result.ok}),
                    )
                )

            history.append(Message(role="tool", function_responses=responses))

        else:
            yield Notice(level="warning", message=f"Reached max steps ({max_steps}).")
            yield AssistantDone(
                turn_id=turn_id,
                text="I hit my step limit. Let me know if you'd like me to continue.",
            )

    async def _execute_tool(
        self, ctx: TurnContext, tool_def: ToolDef, args: dict[str, Any], call_id: str
    ) -> ToolResult:
        policy = check_policy(
            tool_def,
            args,
            tainted=ctx.tainted,
            autonomy=self.config.core.autonomy,
        )

        if policy.decision == Decision.REFUSE:
            audit.log_event(
                "tool_refused",
                turn_id=ctx.turn_id,
                tool=tool_def.name,
                reason=policy.reason,
            )
            return ToolResult(ok=False, content=policy.reason)

        if policy.decision == Decision.ASK:
            approval_id = str(uuid.uuid4())
            summary = tool_def.render_summary(args)

            audit.log_event(
                "approval_requested",
                turn_id=ctx.turn_id,
                approval_id=approval_id,
                tool=tool_def.name,
                summary=summary,
            )

            response = await self.approval_handler.request_approval(
                ApprovalRequest(
                    approval_id=approval_id,
                    tool=tool_def.name,
                    tier=policy.effective_tier.value,
                    summary=summary,
                    args=args,
                    reason=policy.reason,
                    expires_at="",
                )
            )

            audit.log_event(
                "approval_response",
                turn_id=ctx.turn_id,
                approval_id=approval_id,
                decision=response.decision if response else "timeout",
            )

            if not response or response.decision == "deny":
                return ToolResult(ok=False, content="User denied this action.")

            if response.decision == "approve_always":
                from mi_core.permissions.policy import add_allow_rule

                add_allow_rule(tool_def.name)

        try:
            result = await _call_tool_fn(tool_def, args)
        except Exception as e:
            return ToolResult(ok=False, content=f"Tool error: {e}")
        return result

    def cancel(self, turn_id: str) -> None:
        pass


class ApprovalHandler:
    async def request_approval(self, request: ApprovalRequest) -> ApprovalResponse | None:
        raise NotImplementedError


class _DenyAllHandler(ApprovalHandler):
    async def request_approval(self, request: ApprovalRequest) -> ApprovalResponse | None:
        return ApprovalResponse(approval_id=request.approval_id, decision="deny")


class AutoApproveHandler(ApprovalHandler):
    async def request_approval(self, request: ApprovalRequest) -> ApprovalResponse | None:
        return ApprovalResponse(approval_id=request.approval_id, decision="approve")


class WebSocketApprovalHandler(ApprovalHandler):
    def __init__(self) -> None:
        self._pending: dict[str, asyncio.Future[ApprovalResponse]] = {}

    async def request_approval(self, request: ApprovalRequest) -> ApprovalResponse | None:
        fut: asyncio.Future[ApprovalResponse] = asyncio.get_event_loop().create_future()
        self._pending[request.approval_id] = fut
        try:
            return await asyncio.wait_for(fut, timeout=120)
        except TimeoutError:
            return None
        finally:
            self._pending.pop(request.approval_id, None)

    def resolve(self, response: ApprovalResponse) -> bool:
        fut = self._pending.get(response.approval_id)
        if fut and not fut.done():
            fut.set_result(response)
            return True
        return False

    @property
    def pending(self) -> dict[str, asyncio.Future[ApprovalResponse]]:
        return self._pending


async def _call_tool_fn(tool_def: ToolDef, args: dict[str, Any]) -> ToolResult:
    import inspect

    fn = tool_def.fn
    if inspect.iscoroutinefunction(fn):
        result = await fn(**args)
    else:
        result = fn(**args)
    if isinstance(result, ToolResult):
        return result
    return ToolResult(ok=True, content=str(result))


def _preview_args(args: dict[str, Any]) -> str:
    parts = [f"{k}={v!r}" for k, v in args.items()]
    text = ", ".join(parts)
    return text[:100]


def _content_may_taint(tool_def: ToolDef) -> bool:
    tainting = {"files.read", "web.search", "web.fetch", "shell.run", "clipboard.read"}
    return tool_def.name in tainting
