# Build dist\mi.exe: one standalone file (Nuitka). Run from anywhere.
# Uses Nuitka's own MinGW64 (auto-downloaded on first run), so no compiler install is needed.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $root "core")

New-Item -ItemType Directory -Force (Join-Path $root "dist") | Out-Null

uv run python -m nuitka `
    --onefile `
    --mingw64 `
    --assume-yes-for-downloads `
    --windows-console-mode=attach `
    --output-dir="$root\dist\build" `
    --output-filename=mi.exe `
    --include-package=mi_core `
    --include-package=keyring.backends `
    --include-package=uvicorn `
    --include-package=websockets `
    --include-package-data=google.genai `
    --nofollow-import-to=mypy `
    --nofollow-import-to=ruff `
    --nofollow-import-to=nuitka `
    --nofollow-import-to=pytest `
    --nofollow-import-to=respx `
    --nofollow-import-to=_pytest `
    --remove-output `
    --company-name=mi --product-name=mi --file-version=0.1.0 `
    mi_core/main.py

if ($LASTEXITCODE -ne 0) { throw "Nuitka build failed" }
Move-Item -Force "$root\dist\build\mi.exe" "$root\dist\mi.exe"
Write-Host "Built: $root\dist\mi.exe"
