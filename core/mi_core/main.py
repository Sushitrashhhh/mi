"""Entrypoint: `mi` runs the assistant, `mi <cmd>` runs the CLI."""

from __future__ import annotations

from mi_core.cli import app


def main() -> None:
    app()


if __name__ == "__main__":
    main()
