import json
import re
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from pydantic import ValidationError
from starlette.websockets import WebSocketDisconnect
from typer.testing import CliRunner

from mi_core import events as ev
from mi_core.config import ModelsConfig, load_config, set_model
from mi_core.doctor import check_models
from mi_core.llm.base import FakeProvider, Message, ModelInfo
from mi_core.llm.gemini import GeminiProvider
from mi_core.server.auth import load_or_create_token
from mi_core.server.ws import create_app

MODELS = [
    ModelInfo(id="gem-a-flash-lite-2", actions=["generateContent"]),
    ModelInfo(id="gem-a-flash-2", actions=["generateContent"]),
    ModelInfo(id="gem-a-pro-2", actions=["generateContent"]),
    ModelInfo(id="gem-embed-1", actions=["embedContent"]),
]


# ---- config ----
def test_first_run_creates_default_config(mi_home: Path):
    cfg = load_config()
    assert (mi_home / "mi.toml").exists()
    assert cfg.core.host == "127.0.0.1"
    assert cfg.models.default == ""  # model IDs are never baked in


def test_set_model_rewrites_only_models_section(mi_home: Path):
    load_config()
    set_model("default", "some-model")
    cfg = load_config()
    assert cfg.models.default == "some-model"
    assert cfg.models.fast == ""


def test_host_cannot_be_non_localhost(mi_home: Path):
    load_config()
    p = mi_home / "mi.toml"
    p.write_text(p.read_text().replace('host = "127.0.0.1"', 'host = "0.0.0.0"'))
    with pytest.raises(ValidationError):
        load_config()


# ---- events ----
def test_envelope_roundtrip_and_validation():
    e = ev.make_event("user.message", ev.UserMessage(text="hi"))
    again = ev.Envelope.model_validate_json(e.model_dump_json())
    assert isinstance(again.payload(), ev.UserMessage)
    assert re.match(r"^[0-9a-f-]{36}$", again.id)
    with pytest.raises(ValueError):
        ev.Envelope(type="nope").payload()


# ---- server ----
def _client(replies=None):
    token = load_or_create_token()
    app = create_app(load_config(), FakeProvider(replies or ["hello there"]), token)
    return TestClient(app), token


def test_ws_rejects_missing_or_bad_token():
    client, token = _client()
    for url in ("/ws", "/ws?token=wrong"):
        with pytest.raises(WebSocketDisconnect):
            with client.websocket_connect(url):
                pass


def test_ws_streams_reply_with_good_token():
    client, token = _client(["hello there friend"])
    with client.websocket_connect(f"/ws?token={token}") as ws:
        ws.send_text(ev.make_event("user.message", ev.UserMessage(text="hi")).model_dump_json())
        deltas, done = [], None
        while done is None:
            env = ev.Envelope.model_validate_json(ws.receive_text())
            if env.type == "assistant.delta":
                deltas.append(env.data["text"])
            elif env.type == "assistant.done":
                done = env
        assert len(deltas) == 3
        assert done.data["text"].strip() == "hello there friend"


def test_ws_bad_event_returns_error_not_crash():
    client, token = _client()
    with client.websocket_connect(f"/ws?token={token}") as ws:
        ws.send_text("{not json")
        assert ev.Envelope.model_validate_json(ws.receive_text()).type == "error"
        ws.send_text(ev.Envelope(type="user.message", data={}).model_dump_json())
        assert ev.Envelope.model_validate_json(ws.receive_text()).type == "error"


def test_token_file_is_stable(mi_home: Path):
    assert load_or_create_token() == load_or_create_token()


# ---- doctor ----
def test_doctor_flags_unset_invalid_and_ok():
    cfg = ModelsConfig(default="gem-a-flash-2", fast="gem-a-flash-lite-3", deep="")
    by = {c.role: c for c in check_models(cfg, MODELS)}
    assert by["default"].status == "ok"
    assert by["fast"].status == "invalid" and "gem-a-flash-lite-2" in by["fast"].suggestions
    assert by["deep"].status == "unset" and by["deep"].suggestions == ["gem-a-pro-2"]
    assert by["embed"].suggestions == ["gem-embed-1"]


# ---- gemini provider (SDK client mocked) ----
class _Part:
    def __init__(self, text):
        self.text = text


class _Models:
    async def generate_content_stream(self, **kw):
        assert kw["model"] == "m1"

        async def gen():
            for t in ["a", "b", None, "c"]:
                yield _Part(t)

        return gen()

    async def list(self):
        class M:
            def __init__(self, n):
                self.name, self.display_name, self.supported_actions = n, n, ["generateContent"]

        async def gen():
            yield M("models/zeta")
            yield M("models/alpha")

        return gen()


class _Aio:
    models = _Models()


class _Client:
    aio = _Aio()


async def test_gemini_stream_and_list_models():
    p = GeminiProvider("k", client=_Client())
    text = "".join([c.text async for c in p.stream([Message(role="user", text="x")], model="m1")])
    assert text == "abc"
    assert [m.id for m in await p.list_models()] == ["alpha", "zeta"]


async def test_gemini_requires_model():
    p = GeminiProvider("k", client=_Client())
    with pytest.raises(Exception, match="No model configured"):
        [c async for c in p.stream([Message(role="user", text="x")], model="")]


# ---- CLI ----
def test_cli_version_and_no_key_messages():
    from mi_core.cli import app

    r = CliRunner()
    assert "mi 0.1.0" in r.invoke(app, ["version"]).output
    out = r.invoke(app, ["ask", "hi"])
    assert out.exit_code == 1 and "set-gemini-key" in out.output
    out = r.invoke(app, ["doctor", "--json"])
    assert out.exit_code == 1 and json.loads(out.output)["key"] is False


# ---- no secrets in the repo ----
def test_no_api_keys_in_source():
    root = Path(__file__).resolve().parents[2]
    pat = re.compile(r"AIza[0-9A-Za-z_\-]{30,}")
    for f in list(root.rglob("*.py")) + list(root.rglob("*.toml")) + list(root.rglob("*.md")):
        if ".venv" in f.parts:
            continue
        assert not pat.search(f.read_text(encoding="utf-8", errors="ignore")), f
