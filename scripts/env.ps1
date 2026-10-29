# Переменные окружения для сборки Rust/Tauri на этой машине.
# Использование:  . .\scripts\env.ps1
# Причина: Build Tools установлены без регистрации экземпляра Visual Studio,
# Windows SDK взят из NuGet-пакетов в .tools\sdk.

$MsvcRoot = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207'
$SdkRoot = Join-Path $PSScriptRoot '..\.tools\sdk'
$SdkCpp = Join-Path $SdkRoot 'microsoft.windows.sdk.cpp.10.0.28000.2957\c\Include\10.0.28000.0'
$SdkLib = Join-Path $SdkRoot 'microsoft.windows.sdk.cpp.x64.10.0.28000.2957\c'
$SdkTools = Join-Path $SdkRoot 'microsoft.windows.sdk.buildtools.10.0.28000.2957\bin\10.0.28000.0\x64'

$env:INCLUDE = "$MsvcRoot\include;$SdkCpp\ucrt;$SdkCpp\shared;$SdkCpp\um;$SdkCpp\winrt;$SdkCpp\cppwinrt"
$env:LIB = "$MsvcRoot\lib\x64;$SdkLib\ucrt\x64;$SdkLib\um\x64"
$env:CC = "$MsvcRoot\bin\Hostx64\x64\cl.exe"
# tauri-build ищет компилятор ресурсов отдельно — задаём явно.
$env:RC = "$SdkTools\rc.exe"

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$nodeDir = 'C:\Program Files\nodejs'
$env:Path = "$MsvcRoot\bin\Hostx64\x64;$SdkTools;$cargoBin;$nodeDir;" + $env:Path

Write-Host "env: MSVC 14.44.35207 + Windows SDK 10.0.28000.0 подключены" -ForegroundColor DarkGreen
