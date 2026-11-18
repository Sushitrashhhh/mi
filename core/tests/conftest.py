import pytest


@pytest.fixture(autouse=True)
def mi_home(tmp_path, monkeypatch):
    """Every test gets its own ~/.mi and never touches the real keyring/key."""
    monkeypatch.setenv("MI_HOME", str(tmp_path / "mi_home"))
    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    return tmp_path / "mi_home"
