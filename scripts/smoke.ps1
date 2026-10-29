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
    $line = Select-String -Path $log -Pattern 'LEGAMINDSIDECAR READY port=(\d+)' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($line) { $port = [int]$line.Matches[0].Groups[1].Value; break }
}
if (-not $port) { Stop-Process -Id $proc.Id -Force; throw 'Сайдкар не сообщил порт' }

try {
    $health = Invoke-RestMethod "http://127.0.0.1:$port/health" -TimeoutSec 10
    Write-Host "health: $($health.status), загружено: $($health.models_loaded -join ', ')"

    $models = Invoke-RestMethod "http://127.0.0.1:$port/models" -TimeoutSec 10
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
    # Invoke-WebRequest в PowerShell 5.1 падает на chunked SSE — читаем поток вручную.
    $request = [System.Net.HttpWebRequest]::Create("http://127.0.0.1:$port/chat")
    $request.Method = 'POST'
    $request.ContentType = 'application/json'
    $request.Timeout = 300000
    $request.ReadWriteTimeout = 300000
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $request.ContentLength = $bytes.Length
    $stream = $request.GetRequestStream()
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Close()

    $response = $request.GetResponse()
    $reader = New-Object System.IO.StreamReader($response.GetResponseStream(), [System.Text.Encoding]::UTF8)
    $done = $false
    while (-not $reader.EndOfStream) {
        $line = $reader.ReadLine()
        if ($line -and $line.StartsWith('data:')) {
            Write-Host $line
            if ($line -match '"done"') { $done = $true; break }
        }
    }
    $reader.Close()
    $response.Close()

    if ($done) { Write-Host 'SMOKE OK' -ForegroundColor Green } else { Write-Host 'SMOKE: нет события done' -ForegroundColor Red }
}
finally {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Remove-Item $log -Force -ErrorAction SilentlyContinue
}
