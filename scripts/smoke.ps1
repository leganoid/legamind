# Смоук-проверка сайдкара: health, models, короткий чат.
# Файл сохранён в UTF-8 с BOM (иначе PowerShell 5.1 ломает кириллицу).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$python = Join-Path $env:LOCALAPPDATA 'venvs\ai\Scripts\python.exe'
if (-not (Test-Path $python)) { $python = 'python' }

$log = New-TemporaryFile
$proc = Start-Process -FilePath $python -ArgumentList @('-m', 'engine', '--port', '0', '--host', '127.0.0.1') `
    -WorkingDirectory $root -RedirectStandardOutput $log -PassThru -WindowStyle Hidden

$port = $null
for ($i = 0; $i -lt 300; $i++) {
    Start-Sleep -Milliseconds 200
    $line = Select-String -Path $log -Pattern 'HIVESIDECAR READY port=(\d+)' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($line) { $port = [int]$line.Matches[0].Groups[1].Value; break }
}
if (-not $port) { Stop-Process -Id $proc.Id -Force; throw 'Сайдкар не сообщил порт' }

try {
    $health = Invoke-RestMethod "http://127.0.0.1:$port/health"
    Write-Host "health: $($health.status), загружено: $($health.models_loaded -join ', ')"

    $models = Invoke-RestMethod "http://127.0.0.1:$port/models"
    Write-Host "models: $($models.Count)"
    $models | ForEach-Object { Write-Host ("  {0} ({1}, {2} ГБ)" -f $_.id, $_.device, $_.size_gb) }

    $model = $models | Where-Object { $_.id -eq 'qwen-1.5b' } | Select-Object -First 1
    if (-not $model) { $model = $models | Select-Object -First 1 }

    $body = @{
        model       = $model.id
        messages    = @(@{ role = 'user'; content = 'Ответь одним словом: работает?' })
        max_tokens  = 24
        temperature = 0.2
        think       = $false
    } | ConvertTo-Json -Depth 5

    Write-Host "chat ($($model.id))..."
    $response = Invoke-WebRequest -Uri "http://127.0.0.1:$port/chat" -Method Post -Body $body `
        -ContentType 'application/json' -TimeoutSec 300
    $text = ($response.Content -split "`n" | Where-Object { $_ -like 'data: *' }) -join "`n"
    Write-Host $text
    if ($text -match '"done"') { Write-Host 'SMOKE OK' } else { Write-Host 'SMOKE: нет события done' }
}
finally {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Remove-Item $log -Force -ErrorAction SilentlyContinue
}
