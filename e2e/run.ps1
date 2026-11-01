# E2E-прогон Legamind (Windows UI Automation)
#
# Запуск:  powershell -ExecutionPolicy Bypass -File e2e\run.ps1
# Требует установленное приложение C:\Program Files\Legamind\legamind.exe
# и настроенный провайдер DeepSeek.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$python = "$env:LOCALAPPDATA\venvs\ai\Scripts\python.exe"
$appPath = "C:\Program Files\Legamind\legamind.exe"
$project = Join-Path $root ".tools\e2e-project"
$reportPath = Join-Path $root "e2e\last-run.json"
$steps = New-Object System.Collections.Generic.List[object]

function Report($name, $ok, $detail) {
    $script:steps.Add([ordered]@{ step = $name; ok = [bool]$ok; detail = $detail })
    $mark = if ($ok) { "PASS" } else { "FAIL" }
    Write-Host "[$mark] $name — $detail"
}

function Get-Facts {
    $json = & $python "$root\e2e\fixture.py" facts
    return $json | ConvertFrom-Json
}

function Get-Window {
    $proc = Get-Process -Name legamind -ErrorAction SilentlyContinue |
        Where-Object { -not $_.HasExited -and $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
    if (-not $proc) { return [IntPtr]::Zero }
    return $proc.MainWindowHandle
}

function Find-Element($hwnd, $type, $pattern) {
    if (-not $hwnd -or $hwnd -eq [IntPtr]::Zero) { return $null }
    try {
        $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    } catch {
        return $null
    }
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        $type
    )
    foreach ($el in $rootEl.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)) {
        if ($el.Current.Name -like $pattern) { return $el }
    }
    return $null
}

function Invoke-Element($el) {
    if (-not $el) { return $false }
    try {
        $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        return $true
    } catch {
        try {
            $rect = $el.Current.BoundingRectangle
            [E2E]::SetCursorPos([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2)) | Out-Null
            [E2E]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
            [E2E]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
            return $true
        } catch { return $false }
    }
}

function Send-Chat($text) {
    Add-Type -AssemblyName System.Windows.Forms
    $hwnd = Get-Window
    if (-not $hwnd) { throw "окно приложения не найдено" }
    [E2E]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 300
    $edit = Find-Element $hwnd ([System.Windows.Automation.ControlType]::Edit) "*Сообщение*"
    if (-not $edit) { throw "композер не найден" }
    try {
        $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($text)
    } catch {
        $edit.SetFocus()
        Start-Sleep -Milliseconds 200
        [System.Windows.Forms.SendKeys]::SendWait($text)
    }
    Start-Sleep -Milliseconds 500
    $send = Find-Element $hwnd ([System.Windows.Automation.ControlType]::Button) "Отправить"
    if ($send -and -not $send.Current.IsEnabled) { Start-Sleep -Milliseconds 800; $send = Find-Element $hwnd ([System.Windows.Automation.ControlType]::Button) "Отправить" }
    if ($send) {
        Invoke-Element $send | Out-Null
    } else {
        [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    }
    Start-Sleep -Milliseconds 500
}

function Wait-Composer($seconds) {
    for ($i = 0; $i -lt $seconds; $i++) {
        $hwnd = Get-Window
        # тур мог перекрыть интерфейс — закрываем его до проверки композера
        $skip = Find-Element $hwnd ([System.Windows.Automation.ControlType]::Button) "Пропустить"
        if ($skip) { Invoke-Element $skip | Out-Null; Start-Sleep -Milliseconds 500 }
        $edit = Find-Element $hwnd ([System.Windows.Automation.ControlType]::Edit) "*Сообщение*"
        if ($edit) { return $true }
        Start-Sleep -Seconds 1
    }
    return $false
}

function Wait-Element($type, $pattern, $seconds) {
    for ($i = 0; $i -lt $seconds; $i++) {
        $el = Find-Element (Get-Window) $type $pattern
        if ($el) { return $el }
        Start-Sleep -Seconds 1
    }
    return $null
}

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot "Native.cs")

Get-Process -Name legamind -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

$setup = & $python "$root\e2e\fixture.py" setup $project | ConvertFrom-Json
Report "Подготовка сессии и проекта" $true "session=$($setup.session_id)"

Remove-Item (Join-Path $project "e2e-out.txt") -ErrorAction SilentlyContinue
Remove-Item (Join-Path $project "e2e-out2.txt") -ErrorAction SilentlyContinue

# ---------------------------------------------------------------- запуск
$app = Start-Process -FilePath $appPath -PassThru
Report "Запуск приложения" ($null -ne $app) "pid=$($app.Id)"

$composer = Wait-Composer 30
Report "Композер доступен (тур закрыт)" $composer ""
if (-not $composer) {
    Get-Process -Name legamind -ErrorAction SilentlyContinue | Stop-Process -Force
    $steps | ConvertTo-Json -Depth 5 | Set-Content $reportPath -Encoding UTF8
    exit 1
}

# ---------------------------------------------------------------- 1. запись с подтверждением
Send-Chat "Создай e2e-out.txt с текстом E2E_WRITE_111."
$approve = Wait-Element ([System.Windows.Automation.ControlType]::Button) "Разрешить запись" 25
$approved = $false
if ($approve) {
    $remember = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::CheckBox) "*Больше не спрашивать*"
    if ($remember) {
        try { $remember.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle() } catch {}
    }
    $approved = Invoke-Element $approve
}
Start-Sleep -Seconds 6
$file1 = Join-Path $project "e2e-out.txt"
$content1 = if (Test-Path $file1) { (Get-Content $file1 -Raw).Trim() } else { "" }
Report "Запись файла с подтверждением" ($content1 -eq "E2E_WRITE_111") "approved=$approved content=$content1"

$facts = Get-Facts
Report "Разрешение сохранено" ($facts.permissions -contains "write") "permissions=$($facts.permissions -join ',')"
Report "Вызов инструмента привязан к сообщению" $facts.tool_message_linked "last_tool=$($facts.last_tool.tool)"

# ---------------------------------------------------------------- 2. RAG
Send-Chat "/index"
$indexed = $false
for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Seconds 1
    $facts = Get-Facts
    if ($facts.rag_token_chunks -gt 0) { $indexed = $true; break }
}
Report "RAG-индексация проекта" $indexed "chunks=$($facts.chunks) rag_token_chunks=$($facts.rag_token_chunks)"

Send-Chat "Какой уникальный токен в файле notes.md? Ответь одной строкой."
$ragAnswer = $false
for ($i = 0; $i -lt 25; $i++) {
    Start-Sleep -Seconds 2
    $facts = Get-Facts
    if ($facts.assistant -and $facts.assistant -match "E2E_RAG_TOKEN_246") { $ragAnswer = $true; break }
}
Report "Ответ по RAG" $ragAnswer "assistant=$($facts.assistant)"

# ---------------------------------------------------------------- 3. перезапуск
$before = Get-Facts
Get-Process -Name legamind -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3
$app2 = Start-Process -FilePath $appPath -PassThru
$composer2 = Wait-Composer 30
Report "Перезапуск приложения" ($null -ne $app2 -and $composer2) "pid=$($app2.Id)"

$after = Get-Facts
Report "История сохранилась" ($after.messages -ge $before.messages -and $after.messages -gt 0) "messages=$($after.messages)"

# ---------------------------------------------------------------- 4. запись без модалки
Send-Chat "Создай e2e-out2.txt с текстом E2E_WRITE_222."
Start-Sleep -Seconds 2
$modalAfterRestart = $null -ne (Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Button) "Разрешить запись")
$file2 = Join-Path $project "e2e-out2.txt"
$content2 = ""
for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Seconds 1
    if (Test-Path $file2) { $content2 = (Get-Content $file2 -Raw).Trim(); break }
}
Report "Запись после перезапуска без модалки" (($content2 -eq "E2E_WRITE_222") -and (-not $modalAfterRestart)) "modal=$modalAfterRestart content=$content2"

# ---------------------------------------------------------------- 5. самопроверка
$settingsBtn = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Button) "*Настройки*"
if ($settingsBtn) { Invoke-Element $settingsBtn | Out-Null }
Start-Sleep -Seconds 3
$appTab = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Button) "*Приложение*"
if ($appTab) { Invoke-Element $appTab | Out-Null }
Start-Sleep -Seconds 2
$selfCheckBtn = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Button) "*🔍*"
$selfCheck = $false
if ($selfCheckBtn) {
    Invoke-Element $selfCheckBtn | Out-Null
    Start-Sleep -Seconds 4
    $schema = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Text) "схема v*"
    $rag = Find-Element (Get-Window) ([System.Windows.Automation.ControlType]::Text) "проектов:*"
    $selfCheck = ($null -ne $schema) -and ($null -ne $rag)
}
Report "Самопроверка" $selfCheck "schema_found=$([bool]$schema) rag_found=$([bool]$rag)"

# ---------------------------------------------------------------- финал
Get-Process -Name legamind -ErrorAction SilentlyContinue | Stop-Process -Force
& $python "$root\e2e\fixture.py" cleanup | Out-Null
$steps | ConvertTo-Json -Depth 5 | Set-Content $reportPath -Encoding UTF8
$failed = @($steps | Where-Object { -not $_.ok }).Count
Write-Host "`nРезультат: $($steps.Count - $failed) из $($steps.Count) шагов пройдено"
if ($failed -gt 0) { exit 1 }
