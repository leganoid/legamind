# Тесты: Rust (unit + интеграционные) и Python.
# Использование:  powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env.ps1')
$root = Split-Path -Parent $PSScriptRoot
$python = Join-Path $env:LOCALAPPDATA 'venvs\ai\Scripts\python.exe'

Push-Location (Join-Path $root 'src-tauri')
cargo test
$cargoExit = $LASTEXITCODE
Pop-Location

Push-Location $root
& $python -m pytest engine\tests -q
$pytestExit = $LASTEXITCODE
Pop-Location

if ($cargoExit -ne 0 -or $pytestExit -ne 0) { exit 1 }
Write-Host 'Все тесты прошли' -ForegroundColor Green
