import pytest


@pytest.fixture(autouse=True)
def mi_home(tmp_path, monkeypatch):
    """Every test gets its own ~/.mi and never touches the real keyring/key."""
    monkeypatch.setenv("MI_HOME", str(tmp_path / "mi_home"))
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    monkeypatch.setattr("mi_core.secrets.keyring.get_password", lambda *a, **kw: None)
    monkeypatch.setattr("mi_core.secrets.keyring.set_password", lambda *a, **kw: None)
    monkeypatch.setattr("mi_core.secrets.keyring.delete_password", lambda *a, **kw: None)
    return tmp_path / "mi_home"
