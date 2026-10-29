# Legamind

Нативное десктопное приложение для работы с локальными (OpenVINO) и облачными моделями.
Tauri v2 (Rust) + SolidJS + Python-сайдкар `legamind-engine`.

## Что уже работает

| Блок | Состояние |
|---|---|
| M1 — нативное окно, чат со стримингом, сессии в SQLite | готово |
| M2 — облачные провайдеры, ключи в Credential Manager, мок OpenAI | готово |
| M3 — дебаты моделей, анонимизация, голосование, экспорт .md | готово |
| M4 — память: FTS5, «запомни: …», подмешивание в промпт | готово |
| M5 — остановка генерации, переименование/удаление, иконки, сборка MSI | готово; установщик собран и проверен установкой |
| Полировка чата | markdown с подсветкой кода, копирование, перегенерация, редактирование сообщения, статус загрузки модели, индикатор контекста |
| Скиллы и команды | конструктор навыков (роль, инструкции, формат, примеры, ограничения, триггеры), импорт SKILL.md, включение/выключение, слэш-команды `/compact`, `/clear`, `/help`, `/remember`, `/skills`, `/skill`, `/export` |
| Обучение | самообучение без градиентов (👎 с комментарием → правило → system-промпт), датасеты дистилляции у облачной модели-учителя (JSONL), проверка окружения и скрипты LoRA → OpenVINO IR |

## Требования

* Windows 11 x64, WebView2.
* Python 3.12 c OpenVINO GenAI: `%LOCALAPPDATA%\venvs\ai`
  (в нём должны быть `openvino`, `openvino-genai`, `fastapi`, `uvicorn`, `pydantic`).
* Node.js 20+ и npm.
* Rust (stable, MSVC) + MSVC Build Tools + Windows SDK.
* Модели OpenVINO IR в `C:\Models\...` (см. `TASK.md`), кэш компиляции — `C:\Models\cache`.

### Особенности этой машины

`aka.ms` недоступен по HTTPS, поэтому `winget install Microsoft.VisualStudio.2022.BuildTools`
не работает. Порядок, которым всё поставлено:

1. `rustup`, `Node.js` — через `winget`.
2. VS Build Tools — установщик `vs_BuildTools.exe` скачан по **http**-адресу и запущен с
   локальным манифестом канала (`--channelUri http://127.0.0.1:8123/channel.json`).
3. Windows SDK — NuGet-пакеты `Microsoft.Windows.SDK.CPP*` распакованы в `.tools\sdk`
   (VS-инстанс не зарегистрирован в vswhere).
4. Переменные `INCLUDE`/`LIB`/`CC`/`PATH` выставляются скриптом `scripts\env.ps1`
   и продублированы в `.cargo\config.toml`.

## Запуск

```powershell
# 1) зависимости фронтенда (один раз)
npm install

# 2) дев-режим: vite + окно Tauri (скрипт сам подхватывает MSVC-окружение)
powershell -ExecutionPolicy Bypass -File .\scripts\dev.ps1
```

Сборка установщика и тесты:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build.ps1
# → src-tauri\target\release\bundle\msi\Legamind_0.1.0_x64_en-US.msi
powershell -ExecutionPolicy Bypass -File .\scripts\test.ps1
```

> **Важно при запуске из-под агентской песочницы.** Процессы, запущенные из папки проекта,
> могут быть ограничены файловой песочницей (запись только в workspace) — тогда приложение не
> сможет создать БД в `%APPDATA%` и упадёт на WebView2. Скопируйте собранный exe за пределы
> workspace (например, в `%LOCALAPPDATA%\legamind-run\`) или установите `.msi`: установленное
> приложение этой проблемы не имеет. Подробности — в `NOTES.md`.

Сайдкар запускается приложением сам: `python -m engine --port 0 --models-config <appdata>\engine_config.json`,
Rust читает из stdout строку `LEGAMINDSIDECAR READY port=...` и завершает процесс при выходе.
Вывод движка пишется в `%APPDATA%\com.legamind.app\logs\engine_stdout.log`.

## Проверки

```powershell
# Rust: БД, парсер голосования, маскировка ключей
cd src-tauri; cargo test

# Python: реестр, промпты, бюджет памяти
& "$env:LOCALAPPDATA\venvs\ai\Scripts\python.exe" -m pytest engine\tests -q

# Скрипт-смоук сайдкара (health → models → короткий чат)
powershell -ExecutionPolicy Bypass -File .\scripts\smoke.ps1

# Мок OpenAI-совместимого сервера для M2
& "$env:LOCALAPPDATA\venvs\ai\Scripts\python.exe" .\scripts\mock_openai.py
# затем в приложении: Провайдеры → Свой → http://127.0.0.1:8099/v1, ключ любой
```

## Структура

```
src/                 SolidJS UI (чат, дебаты, модели, провайдеры, память, настройки)
src-tauri/src/       db.rs, models.rs (ModelRouter), chat.rs, debate.rs, memory.rs,
                     providers.rs, secrets.rs, sidecar.rs
engine/              FastAPI-сайдкар: registry.py, manager.py, prompts.py, main.py
scripts/             mock_openai.py, smoke.ps1, env.ps1
```

## Безопасность

* Ключи — Windows Credential Manager (`keyring`, service `legamind`), fallback DPAPI.
  В SQLite лежит только `key_ref = provider:<id>`.
* В `Authorization` заголовок добавляется непосредственно перед запросом; в логи и события
  попадает только маска `sk-…ab12`.
* Сеть: `127.0.0.1` и хосты, которые пользователь сам добавил как провайдеров. Никакой телеметрии.
* Собранные промпты пишутся в `%APPDATA%\com.legamind.app\logs\prompts.log` (без ключей).

## Данные

* БД: `%APPDATA%\com.legamind.app\legamind.db`
* Логи: `%APPDATA%\com.legamind.app\logs\`
* Экспорт дебатов: `%APPDATA%\com.legamind.app\exports\debate-<id>.md`

## Интернет для локальных моделей

Локальные модели получают свежие данные из интернета: перед ответом Legamind ищет запрос
и вставляет результаты в system-промпт («Актуальные данные из интернета…»), а ссылки
сохраняются под ответом как «Источники». Включается кнопкой 🌐 в шапке чата или в настройках;
команды — `/web <запрос>`, `/web on|off`.

Провайдеры поиска: Yahoo (без ключа, по умолчанию), Wikipedia, Stack Exchange, GitHub (без
ключа), свой SearxNG, Tavily и Brave (API-ключи), плюс best-effort Bing/DuckDuckGo. Режим
«Авто» перебирает: ключ → SearxNG → Yahoo → Wikipedia → Stack Exchange → GitHub.
Короткие реплики («привет») не ищутся; запрос не длиннее 300 символов.
