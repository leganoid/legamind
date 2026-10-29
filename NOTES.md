# Отклонения от задания и проблемы машины

Документ фиксирует всё, что на этой машине отличается от TASK.md. Архитектура не менялась.

## 1. `aka.ms` недоступен по HTTPS

`winget install Microsoft.VisualStudio.2022.BuildTools` не работает: установщик качает манифест
канала с `https://aka.ms/vs/17/release/channel`, а соединение рвётся (HTTP-вариант при этом
отдаёт 302 на CDN, и `download.visualstudio.microsoft.com` доступен).

Обход, применённый здесь (без изменения задания):

1. `vs_BuildTools.exe` скачан по `http://aka.ms/vs/17/release/vs_BuildTools.exe`.
2. Манифест канала скачан по `http://aka.ms/vs/17/release/channel` и роздан локально:
   `python -m http.server 8123` в `.tools`.
3. Установка: `vs_BuildTools.exe --quiet --wait --add Microsoft.VisualStudio.Workload.VCTools
   --includeRecommended --channelUri http://127.0.0.1:8123/channel.json`.
4. Windows SDK (10.0.28000.0) взят NuGet-пакетами `Microsoft.Windows.SDK.CPP`,
   `Microsoft.Windows.SDK.CPP.x64`, `Microsoft.Windows.SDK.BuildTools` в `.tools/sdk`
   (VS-инстанс не зарегистрирован, `vswhere` его не видит).

## 2. Окружение сборки Rust

Из-за п.1 `rustc` не находит MSVC автоматически. Поэтому:

* `.cargo/config.toml` задаёт линкер, `INCLUDE`, `LIB`, `CC` абсолютными путями;
* `scripts/env.ps1` выставляет то же самое + `PATH` для `rc.exe` и `cl.exe`.

`cargo build` из «чистой» консоли без этих переменных работать не будет — это ограничение машины,
а не проекта.

## 3. Ошибка D8050 при компиляции C-кода

`cl.exe` из Build Tools 14.44 падал с
`error D8050: нельзя выполнить c1.dll: не удалось поместить командную строку в запись отладки`
при компиляции `sqlite3.c`/`ring` через `cc` с `/Z7` (отладочная информация C).
Решение: `[profile.dev] debug = 0` в `src-tauri/Cargo.toml` (для C-кода отладочная информация
выключена; на разработку это не влияет).

## 4. `reqwest` без rustls

В `TASK.md` указаны только фичи `json`, `stream`. Собрано именно так (TLS — системный,
через `native-tls`/SChannel): это и ближе к заданию, и не тянет `ring` с C-сборкой.

## 5. Node.js 26 вместо 22

`winget` в канале LTS отдал Node.js 26.13.0. Требование «20+» выполнено, Vite 7 и Tauri CLI 2.12
работают.

## 6. `tauri-cli` из npm

`cargo install tauri-cli` не выполнялся: тот же CLI (`tauri-cli 2.12.4`) приходит как
`@tauri-apps/cli` в devDependencies, и `npx tauri dev` / `npx tauri build` — его точные аналоги.

## 7. Запуск приложения из-под песочницы DSH

Обнаружено: процессы, запущенные **из папки проекта**, в этой сессии ограничены файловой
песочницей DSH (запись только в workspace). Из-за этого приложение падало на создании БД в
`%APPDATA%` и на WebView2 (ошибки `os error 5`, `0x800700AA`, `0x8000FFFF`).
Собранный exe, скопированный за пределы workspace (и установленный `.msi` из `Program Files`),
работает нормально. Это ограничение среды разработки, а не приложения; в README описан обход.

## 8. Зависание загрузки тяжёлых моделей (исправлено)

В `engine/manager.py` `ensure_loaded` брал `_global_lock`, а затем `_evict_for` брал его повторно —
`threading.Lock` нереентерабельный, поэтому загрузка любой модели тяжелее 8 ГБ (14B, 30B) вставала
навсегда (лёгкие модели не задевали ветку выгрузки). Исправлено: `_evict_for_locked` вызывается
только под уже взятым локом. Проверено: `qwen3-14b` грузится за 5–8 с (кэш), отвечает 2–3 с.

## 9. Вывод сайдкара

Изначально stdout/stderr сайдкара читались пайпом. После строки `READY` читатель закрывался,
и любая последующая запись в stdout могла заблокировать процесс (переполненный пайп).
Сейчас stdout/stderr перенаправлены в файлы `logs/engine_stdout.log` и `logs/engine_stderr.log`,
а порт читается из файла — блокировка исключена.

## 10. Мелочи

* `/embeddings` возвращает 400: в MVP память работает на FTS5, эмбеддинги — этап M3
  «если останется время» (TASK.md, 7.2).
* Qwen3 в think-режиме иногда оставляет пустой блок `<think></think>` в ответе — косметика,
  модель отвечает по существу.
* Иконки сгенерированы программно (`src-tauri/icons`), `cargo tauri icon` не использовался —
  эквивалентный результат без лишней зависимости.
