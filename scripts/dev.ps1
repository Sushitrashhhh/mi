# Run mi in the foreground for development.
Set-Location (Join-Path (Split-Path -Parent $PSScriptRoot) "core")
uv run mi serve
