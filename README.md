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
| Скиллы и команды | конструктор навыков (роль, инструкции, формат, примеры, ограничения, триггеры), импорт SKILL.md, включение/выключение, слэш-команды `/compact`, `/clear`, `/help`, `/remember`, `/skills`, `/skill`, `/export`, `/index` |
| Обучение | самообучение без градиентов (👎 с комментарием → правило → system-промпт), датасеты дистилляции у облачной модели-учителя (JSONL), проверка окружения и скрипты LoRA → OpenVINO IR |
| M6 — инструменты | `list_dir`, `read_file`, `write_file`, `apply_patch` (в т.ч. несколько фрагментов), `delete_file`, `rename_file`, `run_command`; песочница рабочей папки, подтверждение с diff, `audit.log`, бэкапы и «↩ Отменить», guard опасных команд |
| M7 — RAG | локальный полнотекстовый индекс рабочей папки (FTS5), топ-5 фрагментов в промпте, кнопка «📚 Индекс» и `/index` |
| Сессии | рабочая папка проекта, режимы чат/планирование/сборка/ревизия, доверие read_only/workspace/full, уровень рассуждений, проверка орфографии, DeepSeek как облачный провайдер |

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

## Интерфейс

* **Ctrl+K** — command palette: переход по разделам, новая сессия, папка данных.
* **Ctrl+N** — новая сессия чата; `/help` в композере — список слэш-команд.
* В чате: «⚙ Параметры» (режим, обдумывание, доступ к файлам) и «Проект» (папка, индекс,
  история инструментов, отмена изменения, RAG).
* Раздел **«Помощь»** — короткий FAQ: рабочая папка, доступ, бэкапы, горячие клавиши.
* Настройки разбиты на вкладки «Интернет / Приложение / Приватность»; выбранная вкладка
  запоминается.

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
src/                 SolidJS UI (чат, дебаты, модели, провайдеры, память, навыки, обучение, настройки)
src-tauri/src/       db.rs, models.rs (ModelRouter), chat.rs, debate.rs, memory.rs,
                     providers.rs, secrets.rs, sidecar.rs, workspace.rs, web.rs,
                     skills.rs, learning.rs, tools.rs, rag.rs
engine/              FastAPI-сайдкар: registry.py, manager.py, prompts.py, main.py
scripts/             mock_openai.py, smoke.ps1, env.ps1, finetune_qlora.py, export_openvino.py
```

## Безопасность

* Ключи — Windows Credential Manager (`keyring`, service `legamind`), fallback DPAPI.
  В SQLite лежит только `key_ref = provider:<id>`.
* В `Authorization` заголовок добавляется непосредственно перед запросом; в логи и события
  попадает только маска `sk-…ab12`.
* Сеть: `127.0.0.1` и хосты, которые пользователь сам добавил как провайдеров. Никакой телеметрии.
* Собранные промпты пишутся в `%APPDATA%\com.legamind.app\logs\prompts.log` (без ключей).
* Инструменты файлов работают только в рабочей папке (при доверии `workspace`), запись/удаление/
  перемещение подтверждаются модалкой с diff, опасные команды блокируются, все действия — в
  `audit.log`; перед изменением файл копируется в `.legamind/backups`, есть «↩ Отменить».


## Данные

* БД: `%APPDATA%\com.legamind.app\legamind.db`
* Логи: `%APPDATA%\com.legamind.app\logs\`
* Автокопии и ручные копии: `%APPDATA%\com.legamind.app\backups\` — **шифруются DPAPI**
  (открываются только под вашей учётной записью Windows)
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

## Инструменты и RAG

Если у сессии задана рабочая папка, модель может вызывать инструменты JSON-блоком:

```tool
{"tool":"apply_patch","path":"src/main.rs","old":"старый фрагмент","new":"новый фрагмент"}
```

Доступны `list_dir`, `read_file`, `write_file`, `apply_patch` (в том числе `patches: [...]`),
`delete_file`, `rename_file`, `run_command`. Чтение и список — сразу; запись, правка, удаление и
перемещение — через модалку с diff/командой. Доверие `read_only` запрещает изменения,
`workspace` — только внутри папки, `full` разрешает абсолютные пути. Опасные команды
(`format`, `diskpart`, `reg delete`, `rm -rf /` и т.п.) блокируются, потенциально
разрушительные — показываются с предупреждением.

После вызовов модель помнит последние 5 операций (таблица `tool_runs`), в панели сессии есть
«🧰 История», «↩ Отменить» (бэкапы в `.legamind/backups`) и «📚 Индекс».

RAG локальный, без внешних моделей: `/index` или кнопка «📚 Индекс» индексирует текстовые файлы
рабочей папки в FTS5, а перед ответом в промпт добавляются топ-5 релевантных фрагментов с
путями. Индекс привязан к рабочей папке (разные проекты не перетирают друг друга) и обновляется
после изменений через инструменты; правки вне приложения требуют повторного `/index`.
