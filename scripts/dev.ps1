# Dev-запуск: окружение MSVC + vite + окно Tauri.
# Использование:  powershell -ExecutionPolicy Bypass -File .\scripts\dev.ps1

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env.ps1')
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
& npx tauri dev
exit $LASTEXITCODE
