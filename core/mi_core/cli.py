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
from mi_core.config import MiConfig, config_path, load_config, set_model
from mi_core.llm.base import LLMProvider, Message
from mi_core.llm.gemini import GeminiProvider
from mi_core.secrets import get_gemini_key, set_gemini_key
from mi_core.server.ws import SYSTEM_PROMPT

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


async def _stream_to_console(provider: LLMProvider, cfg: MiConfig, history: list[Message]) -> str:
    full = ""
    async for chunk in provider.stream(history, model=cfg.models.default, system=SYSTEM_PROMPT):
        if chunk.text:
            full += chunk.text
            console.out(chunk.text, end="", highlight=False)
    console.out("")
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
    """One-shot question; streams the answer."""
    cfg = load_config()
    provider = _provider()
    try:
        asyncio.run(_stream_to_console(provider, cfg, [Message(role="user", text=prompt)]))
    except Exception as e:
        console.print(f"[red]Error:[/red] {e}")
        raise typer.Exit(1) from e


@app.command()
def chat() -> None:
    """Interactive chat. Ctrl+C or an empty line to quit."""
    cfg = load_config()
    provider = _provider()
    history: list[Message] = []
    console.print("[dim]mi chat. Empty line to quit.[/dim]")
    while True:
        try:
            line = console.input("[bold cyan]you>[/bold cyan] ").strip()
        except (EOFError, KeyboardInterrupt):
            break
        if not line:
            break
        history.append(Message(role="user", text=line))
        try:
            reply = asyncio.run(_stream_to_console(provider, cfg, history))
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
            history.pop()
            continue
        history.append(Message(role="assistant", text=reply))


@app.command()
def serve() -> None:
    """Run the local WebSocket server (foreground)."""
    import uvicorn

    from mi_core.server.auth import load_or_create_token
    from mi_core.server.ws import create_app

    cfg = load_config()
    provider = _provider()
    token = load_or_create_token()
    console.print(f"mi listening on ws://{cfg.core.host}:{cfg.core.port}/ws (token in ~/.mi/token)")
    uvicorn.run(
        create_app(cfg, provider, token),
        host=cfg.core.host,
        port=cfg.core.port,
        log_level="warning",
    )


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
