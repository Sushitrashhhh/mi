"""Register all built-in tool packs."""

from __future__ import annotations


def register_all() -> None:
    import mi_core.tools.builtin.apps  # noqa: F401
    import mi_core.tools.builtin.files  # noqa: F401
    import mi_core.tools.builtin.shell  # noqa: F401
    import mi_core.tools.builtin.system  # noqa: F401
    import mi_core.tools.builtin.timers  # noqa: F401
    import mi_core.tools.builtin.web  # noqa: F401
