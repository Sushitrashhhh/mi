"""CLI (typer). Same binary as the assistant. Read commands support --json where noted."""

from __future__ import annotations

import asyncio
import getpass
import json
from typing import Annotated, Any

import typer
from rich.console import Console

from mi_core import __version__
from mi_core import doctor as doctor_mod
from mi_core.agent.loop import SYSTEM_PROMPT, AgentLoop, AutoApproveHandler
from mi_core.config import MiConfig, config_path, load_config, set_model
from mi_core.llm.base import LLMProvider, Message
from mi_core.llm.gemini import GeminiProvider
from mi_core.secrets import get_gemini_key, set_gemini_key
from mi_core.tools.builtin import register_all
from mi_core.tools.registry import get_registry, reset_registry

app = typer.Typer(help="mi: your system assistant.", no_args_is_help=False, add_completion=False)
auth_app = typer.Typer(help="Store credentials in the OS keyring.")
app.add_typer(auth_app, name="auth")
console = Console()


def _provider() -> GeminiProvider:
    key = get_gemini_key()
    if not key:
        console.print("[red]No Gemini key.[/red] Run: [bold]mi auth set-gemini-key[/bold]")
        raise typer.Exit(1)
    return GeminiProvider(key)


def _setup_registry() -> None:
    reset_registry()
    register_all()


async def _stream_to_console(provider: LLMProvider, cfg: MiConfig, history: list[Message]) -> str:
    full = ""
    async for chunk in provider.stream(history, model=cfg.models.default, system=SYSTEM_PROMPT):
        if chunk.text:
            full += chunk.text
            console.out(chunk.text, end="", highlight=False)
    console.out("")
    return full


async def _agent_to_console(agent: AgentLoop, text: str, history: list[Message]) -> str:
    from mi_core.events import AssistantDelta, AssistantDone, Notice, ToolFinished, ToolStarted

    full = ""
    async for event in agent.run(text, history):
        if isinstance(event, AssistantDelta):
            full += event.text
            console.out(event.text, end="", highlight=False)
        elif isinstance(event, AssistantDone):
            if not full:
                full = event.text
                console.out(event.text, end="", highlight=False)
            console.out("")
        elif isinstance(event, ToolStarted):
            console.print(f"[dim]⚙ {event.tool}({event.args_preview})[/dim]")
        elif isinstance(event, ToolFinished):
            status = "[green]✓[/green]" if event.ok else "[red]✗[/red]"
            console.print(f"[dim]  {status} {event.tool}[/dim]")
        elif isinstance(event, Notice):
            console.print(f"[yellow]{event.message}[/yellow]")
    return full


@app.callback(invoke_without_command=True)
def root(ctx: typer.Context) -> None:
    if ctx.invoked_subcommand is None:
        # `mi` with no args = run the assistant. Phase 3 adds tray + overlay; for now, the server.
        serve()


@app.command()
def version() -> None:
    """Print the mi version."""
    console.out(f"mi {__version__}")


@app.command()
def ask(prompt: Annotated[str, typer.Argument(help="What to ask mi.")]) -> None:
    """One-shot question; streams the answer, can use tools."""
    cfg = load_config()
    provider = _provider()
    _setup_registry()
    agent = AgentLoop(cfg, provider, get_registry(), AutoApproveHandler())
    try:
        asyncio.run(_agent_to_console(agent, prompt, []))
    except Exception as e:
        console.print(f"[red]Error:[/red] {e}")
        raise typer.Exit(1) from e


@app.command()
def chat() -> None:
    """Interactive chat with tools. Ctrl+C or an empty line to quit."""
    cfg = load_config()
    provider = _provider()
    _setup_registry()
    agent = AgentLoop(cfg, provider, get_registry(), AutoApproveHandler())
    history: list[Message] = []
    console.print("[dim]mi chat. Empty line to quit.[/dim]")
    while True:
        try:
            line = console.input("[bold cyan]you>[/bold cyan] ").strip()
        except (EOFError, KeyboardInterrupt):
            break
        if not line:
            break
        try:
            asyncio.run(_agent_to_console(agent, line, history))
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
            continue


@app.command()
def serve() -> None:
    """Run the local WebSocket server (foreground)."""
    import uvicorn

    from mi_core.server.auth import load_or_create_token
    from mi_core.server.ws import create_app

    cfg = load_config()
    provider = _provider()
    _setup_registry()
    token = load_or_create_token()
    console.print(f"mi listening on ws://{cfg.core.host}:{cfg.core.port}/ws (token in ~/.mi/token)")
    uvicorn.run(
        create_app(cfg, provider, token, get_registry()),
        host=cfg.core.host,
        port=cfg.core.port,
        log_level="warning",
    )


@app.command()
def logs(
    tail: Annotated[int, typer.Option(help="Number of recent entries.")] = 20,
    as_json: Annotated[bool, typer.Option("--json", help="Machine-readable.")] = False,
) -> None:
    """Show the audit log (most recent entries)."""
    from mi_core.agent.audit import read_log

    entries = read_log(tail=tail)
    if as_json:
        console.out(json.dumps(entries, indent=2))
        return
    if not entries:
        console.print("[dim]No audit log entries yet.[/dim]")
        return
    for entry in entries:
        ts = entry.get("ts", "?")[:19]
        kind = entry.get("kind", "?")
        rest = {k: v for k, v in entry.items() if k not in ("ts", "kind")}
        detail = " ".join(f"{k}={v}" for k, v in rest.items())
        console.print(f"[dim]{ts}[/dim] [bold]{kind:20}[/bold] {detail[:100]}")


@app.command()
def doctor(
    fix: Annotated[
        bool, typer.Option(help="Fill empty/invalid model slots with suggestions.")
    ] = False,
    as_json: Annotated[bool, typer.Option("--json", help="Machine-readable output.")] = False,
) -> None:
    """Check key, config and model IDs against what your Gemini key can actually use."""
    result: dict[str, Any] = {"config": str(config_path()), "key": False, "roles": [], "ok": False}
    cfg = load_config()
    key = get_gemini_key()
    result["key"] = bool(key)
    checks: list[doctor_mod.RoleCheck] = []
    available: list[Any] = []
    error: str | None = None
    if key:
        try:
            available = asyncio.run(GeminiProvider(key).list_models())
            checks = doctor_mod.check_models(cfg.models, available)
        except Exception as e:
            error = str(e)
    if fix and checks:
        for c in checks:
            if c.status != "ok" and c.suggestions:
                set_model(c.role, c.suggestions[0])
        cfg = load_config()
        checks = doctor_mod.check_models(cfg.models, available)
    result["roles"] = [c.__dict__ for c in checks]
    result["error"] = error
    result["ok"] = bool(key) and error is None and all(c.status == "ok" for c in checks)
    if as_json:
        console.out(json.dumps(result, indent=2))
        raise typer.Exit(0 if result["ok"] else 1)

    console.print(f"Config: {config_path()}")
    if not key:
        console.print("[red]✗ Gemini key not set.[/red] Run: mi auth set-gemini-key")
        raise typer.Exit(1)
    console.print("[green]✓[/green] Gemini key found")
    if error:
        console.print(f"[red]✗ Could not list models:[/red] {error}")
        raise typer.Exit(1)
    console.print(f"[green]✓[/green] {len(available)} models available to your key")
    for c in checks:
        if c.status == "ok":
            console.print(f"[green]✓[/green] {c.role:8} {c.configured}")
        else:
            label = "not set" if c.status == "unset" else f"invalid ({c.configured})"
            sugg = ", ".join(c.suggestions) or "no suggestion"
            console.print(f"[yellow]![/yellow] {c.role:8} {label}. Try: {sugg}")
    if not result["ok"]:
        console.print(
            "\nRun [bold]mi doctor --fix[/bold] to fill these in, or edit the file above."
        )
        raise typer.Exit(1)
    console.print("\nAll good.")


@auth_app.command("set-gemini-key")
def auth_set_gemini_key() -> None:
    """Prompt (hidden) for your Gemini API key and store it in the OS keyring."""
    key = getpass.getpass("Gemini API key (input hidden): ").strip()
    if not key:
        console.print("[red]Empty key, nothing saved.[/red]")
        raise typer.Exit(1)
    set_gemini_key(key)
    console.print("[green]Saved to your OS keyring.[/green] Next: mi doctor --fix")
