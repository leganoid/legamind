# Сборка установщика .msi.
# Использование:  powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env.ps1')
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
& npx tauri build
exit $LASTEXITCODE
