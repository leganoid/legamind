# РЎР±РѕСЂРєР° Р°РІС‚РѕРЅРѕРјРЅРѕРіРѕ СЃР°Р№РґРєР°СЂР° legamind-engine.exe (Windows).
# РСЃРїРѕР»СЊР·РѕРІР°РЅРёРµ: powershell -ExecutionPolicy Bypass -File build-sidecar.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSCommandPath
if ($env:LEGAMIND_PYTHON) {
    $python = $env:LEGAMIND_PYTHON
    if (-not (Get-Command $python -ErrorAction SilentlyContinue) -and -not (Test-Path $python)) {
        throw "Python РЅРµ РЅР°Р№РґРµРЅ: $python"
    }
} else {
    $python = "$env:LOCALAPPDATA\venvs\ai\Scripts\python.exe"
    if (-not (Test-Path $python)) {
        throw "Python venv РЅРµ РЅР°Р№РґРµРЅ: $python"
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
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller Р·Р°РІРµСЂС€РёР»СЃСЏ СЃ РєРѕРґРѕРј $LASTEXITCODE" }
} finally {
    Pop-Location
}

$exe = Join-Path $dist "legamind-engine.exe"
if (-not (Test-Path $exe)) { throw "РќРµ РЅР°Р№РґРµРЅ $exe" }
$size = [math]::Round(((Get-ChildItem $dist -Recurse | Measure-Object Length -Sum).Sum / 1GB), 2)
Write-Host "Р“РѕС‚РѕРІРѕ: $exe (РєР°С‚Р°Р»РѕРі ~$size Р“Р‘)"

Set-Content -Path (Join-Path $dist '.gitkeep') -Value 'placeholder: build-sidecar fills this directory' -Encoding UTF8
