# Third-Party Notices

Legamind использует открытые компоненты. Собственный код проекта распространяется по
лицензии MIT (см. `LICENSE`). Ниже — основные компоненты, которые входят в установщик
или распространяются вместе с приложением.

Полные тексты лицензий — в каталоге [`licenses/`](licenses/).

## Движок (внутри `legamind-engine.exe`)

| Компонент | Версия | Лицензия | Правообладатель / источник |
|---|---|---|---|
| OpenVINO | 2026.4.1 | Apache-2.0 | Intel Corporation, https://github.com/openvinotoolkit/openvino |
| OpenVINO GenAI | 2026.4.1 | Apache-2.0 | Intel Corporation, https://github.com/openvinotoolkit/openvino.genai |
| OpenVINO Tokenizers | 2026.4.1 | Apache-2.0 | Intel Corporation |
| Python | 3.12 | PSF-2.0 | Python Software Foundation, https://www.python.org |
| PyInstaller (bootloader) | 6.22 | GPL-2.0-or-later with special exception | https://pyinstaller.org — исключение разрешает распространять несвободные приложения |
| NumPy | 2.5 | BSD-3-Clause | NumPy Developers |
| FastAPI | 0.162 | MIT | Sebastián Ramírez |
| Uvicorn | 0.54 | BSD-3-Clause | Encode OSS Ltd |
| Pydantic | 2.x | MIT | Pydantic Services Inc. |
| Starlette | 0.4x | BSD-3-Clause | Encode OSS Ltd |

## Приложение

| Компонент | Лицензия | Источник |
|---|---|---|
| Tauri | MIT / Apache-2.0 | https://tauri.app |
| SolidJS | MIT | https://www.solidjs.com |
| marked | MIT | https://github.com/markedjs/marked |
| marked-highlight | MIT | https://github.com/markedjs/marked-highlight |
| highlight.js | BSD-3-Clause | https://highlightjs.org |
| DOMPurify | Apache-2.0 / MPL-2.0 | https://github.com/cure53/DOMPurify |
| SQLite | Public Domain | https://sqlite.org |
| Rust crates (reqwest, serde, tokio, rusqlite, keyring, notify, pdf-extract, ignore и др.) | преимущественно MIT / Apache-2.0 | см. `src-tauri/Cargo.lock` |

## Важно для релиза

- При сборке установщика приложите каталог `licenses/` и этот файл.
- Полный список транзитивных зависимостей и их лицензий можно получить из
  `src-tauri/Cargo.lock` и `package-lock.json`; перед публичным релизом рекомендуется
  сгенерировать полный отчёт (например, `cargo-about` и `license-checker`).
- Если вы добавляете новые библиотеки, обновляйте этот файл.
