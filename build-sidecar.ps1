# Сборка автономного сайдкара legamind-engine.exe (Windows).
# Использование: powershell -ExecutionPolicy Bypass -File build-sidecar.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSCommandPath
if ($env:LEGAMIND_PYTHON) {
    $python = $env:LEGAMIND_PYTHON
    if (-not (Get-Command $python -ErrorAction SilentlyContinue) -and -not (Test-Path $python)) {
        throw "Python не найден: $python"
    }
} else {
    $python = "$env:LOCALAPPDATA\venvs\ai\Scripts\python.exe"
    if (-not (Test-Path $python)) {
        throw "Python venv не найден: $python"
    }
}
$dist = Join-Path $root "sidecar-dist\legamind-engine"
$work = Join-Path $root ".tools\pyinstaller-work"

Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work | Out-Null

Push-Location $root
try {
    & $python -m PyInstaller `
        --noconfirm --clean --onedir --console `
        --name legamind-engine `
        --distpath (Join-Path $root "sidecar-dist") `
        --workpath $work `
        --specpath $work `
        --paths $root `
        --collect-all openvino_genai `
        --collect-all openvino `
        --collect-all openvino_tokenizers `
        --hidden-import uvicorn `
        --hidden-import fastapi `
        --hidden-import pydantic `
        "engine\sidecar_entry.py"
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller завершился с кодом $LASTEXITCODE" }
} finally {
    Pop-Location
}

$exe = Join-Path $dist "legamind-engine.exe"
if (-not (Test-Path $exe)) { throw "Не найден $exe" }
$size = [math]::Round(((Get-ChildItem $dist -Recurse | Measure-Object Length -Sum).Sum / 1GB), 2)
Write-Host "Готово: $exe (каталог ~$size ГБ)"
