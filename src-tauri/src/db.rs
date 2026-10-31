//! SQLite: миграции и запросы.
//!
//! Файл БД: `%APPDATA%\com.legamind.app\legamind.db`.
//! Схема — из TASK.md раздел 8, апгрейды последовательные через `schema_version`.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub created_at: String,
    pub updated_at: String,
    /// Рабочая папка проекта сессии (может быть не задана).
    pub workdir: Option<String>,
    /// Режим: chat | plan | build | review.
    pub mode: String,
    /// Доверие к файловым операциям: read_only | workspace | full.
    pub trust: String,
    /// Уровень размышлений: off/on (локальные), low/high/max (DeepSeek), None — авто.
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRow {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub model_id: Option<String>,
    pub tokens: Option<i64>,
    pub created_at: String,
}

/// Результат поиска по сообщениям (для боковой панели).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHitRow {
    pub session_id: String,
    pub session_title: String,
    pub message_id: String,
    pub role: String,
    pub snippet: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub source_session: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub base_url: String,
    pub key_ref: String,
    pub enabled: bool,
    pub timeout_s: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelRow {
    pub provider_id: String,
    pub model_id: String,
    pub label: Option<String>,
    pub context: Option<i64>,
    /// JSON-массив уровней рассуждений, которые рекламирует провайдер.
    pub efforts: Option<String>,
    pub updated_at: String,
}

/// Навык: набор инструкций, который подмешивается в system-промпт.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub role: Option<String>,
    pub guidelines: Option<String>,
    pub output_format: Option<String>,
    pub examples: Option<String>,
    pub constraints: Option<String>,
    pub triggers: Option<String>,
    pub enabled: bool,
    pub builtin: bool,
    pub created_at: String,
}

/// Усвоенное правило — результат обратной связи (самообучение без градиентов).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonRow {
    pub id: String,
    pub text: String,
    pub source: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub rating: i64,
    pub note: Option<String>,
    pub created_at: String,
}

/// Датасет дистилляции для дообучения (JSONL с парами instruction/answer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub teacher_model: Option<String>,
    pub examples: i64,
    pub created_at: String,
}

/// Источник из интернета, на который опирался ответ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub title: Option<String>,
    pub url: String,
    pub snippet: Option<String>,
    pub created_at: String,
}

/// Один вызов инструмента M6 в рамках сессии.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRunRow {
    pub id: String,
    pub session_id: String,
    pub request_id: Option<String>,
    pub tool: String,
    pub path: Option<String>,
    pub ok: bool,
    pub summary: Option<String>,
    pub output: Option<String>,
    pub created_at: String,
}

/// Резервная копия файла перед изменением (для отмены).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBackupRow {
    pub id: String,
    pub session_id: String,
    pub request_id: Option<String>,
    pub tool: String,
    pub action: String,
    pub path: String,
    pub target: Option<String>,
    pub backup_path: Option<String>,
    pub created: bool,
    pub undone: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagChunkRow {
    pub id: String,
    pub path: String,
    pub chunk_index: i64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagHitRow {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagStats {
    pub chunks: i64,
    pub files: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateRow {
    pub id: String,
    pub task: String,
    pub participants: String,
    pub rounds: i64,
    pub judge: Option<String>,
    pub verdict: Option<String>,
    pub final_answer: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateTurnRow {
    pub id: String,
    pub debate_id: String,
    pub round: i64,
    pub role: String,
    pub model_id: String,
    pub content: String,
    pub tokens: Option<i64>,
    pub seconds: Option<f64>,
    pub created_at: String,
}

/// Обёртка над соединением: одно соединение под мьютексом, как и требует однопользовательское приложение.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .map_err(|e| anyhow::anyhow!("не удалось открыть {}: {e}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Согласованная копия базы (SQLite backup API, учитывает WAL).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = self.conn();
        let mut target = Connection::open(dest)?;
        let backup = rusqlite::backup::Backup::new(&conn, &mut target)?;
        backup.run_to_completion(128, std::time::Duration::from_millis(2), None)?;
        Ok(())
    }

    /// Восстанавливает базу из копии в открытое соединение (без подмены файла).
    pub fn restore_from(&self, source: &Path) -> Result<()> {
        if !source.is_file() {
            anyhow::bail!("Файл копии не найден");
        }
        let src = Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let tables: i64 = src.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name IN ('sessions', 'messages', 'schema_version')",
            [],
            |row| row.get(0),
        )?;
        if tables < 2 {
            anyhow::bail!("Это не похоже на базу Legamind");
        }

        let mut conn = self.conn();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        {
            let backup = rusqlite::backup::Backup::new(&src, &mut conn)?;
            backup.run_to_completion(128, std::time::Duration::from_millis(2), None)?;
        }
        drop(conn);
        self.migrate()?;
        Ok(())
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);")?;
        let current: i64 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |row| row.get(0))
            .unwrap_or(0);

        if current < 1 {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS sessions (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  model_id TEXT NOT NULL,
                  created_at TEXT NOT NULL,
                  updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS messages (
                  id TEXT PRIMARY KEY,
                  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                  role TEXT NOT NULL,
                  content TEXT NOT NULL,
                  model_id TEXT,
                  tokens INTEGER,
                  created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

                CREATE TABLE IF NOT EXISTS memories (
                  id TEXT PRIMARY KEY,
                  kind TEXT NOT NULL,
                  text TEXT NOT NULL,
                  source_session TEXT,
                  created_at TEXT NOT NULL
                );

                CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, content='memories', content_rowid='rowid');

                CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                  INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                END;
                CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                  INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
                END;

                CREATE TABLE IF NOT EXISTS debates (
                  id TEXT PRIMARY KEY,
                  task TEXT NOT NULL,
                  participants TEXT NOT NULL,
                  rounds INTEGER NOT NULL,
                  judge TEXT,
                  verdict TEXT,
                  final_answer TEXT,
                  created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS debate_turns (
                  id TEXT PRIMARY KEY,
                  debate_id TEXT NOT NULL REFERENCES debates(id) ON DELETE CASCADE,
                  round INTEGER NOT NULL,
                  role TEXT NOT NULL,
                  model_id TEXT NOT NULL,
                  content TEXT NOT NULL,
                  tokens INTEGER,
                  seconds REAL,
                  created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS providers (
                  id TEXT PRIMARY KEY,
                  kind TEXT NOT NULL,
                  label TEXT NOT NULL,
                  base_url TEXT NOT NULL,
                  key_ref TEXT NOT NULL,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  timeout_s INTEGER NOT NULL DEFAULT 120,
                  created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS provider_models (
                  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                  model_id TEXT NOT NULL,
                  label TEXT,
                  context INTEGER,
                  updated_at TEXT NOT NULL,
                  PRIMARY KEY (provider_id, model_id)
                );

                CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }

        if current < 2 {
            // Навыки: конструктор хранит части отдельно, в промпт собирается блок.
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS skills (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  description TEXT,
                  role TEXT,
                  guidelines TEXT,
                  output_format TEXT,
                  examples TEXT,
                  constraints TEXT,
                  triggers TEXT,
                  enabled INTEGER NOT NULL DEFAULT 0,
                  builtin INTEGER NOT NULL DEFAULT 0,
                  created_at TEXT NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
                "#,
            )?;
            let seeded: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
                (
                    "Кратко",
                    "Отвечай максимально сжато: 3–5 предложений, без вступлений и повторов.",
                    "Ты — предельно лаконичный ассистент.",
                    "Сформулируй суть в 1–2 предложениях, затем — не более трёх пунктов, если они нужны.",
                    "1–2 предложения, при необходимости — до трёх коротких пунктов.",
                    "Не пересказывай вопрос, не добавляй вводных фраз и извинений.",
                    "",
                ),
                (
                    "Код-ревью",
                    "Разбор кода: риски, ошибки, улучшения — по приоритету.",
                    "Ты — строгий, но доброжелательный ревьюер кода.",
                    "Сначала перечисли реальные проблемы (ошибки, гонки, утечки, безопасность), затем улучшения.\n\
                     Для каждой проблемы укажи: что не так, почему это важно, как исправить (с примером кода).",
                    "Список проблем с приоритетами (critical / major / minor), затем итог и готовый исправленный фрагмент.",
                    "Не придирайся к стилю, если нет функциональной проблемы. Не выдумывай код, которого не видел.",
                    "",
                ),
                (
                    "Тьютор",
                    "Объясняй по шагам, проверяй понимание, давай упражнения.",
                    "Ты — терпеливый преподаватель.",
                    "Разбей тему на шаги, объясняй простыми словами, после каждого шага задай один проверочный вопрос.\n\
                     В конце предложи небольшое упражнение.",
                    "Короткие абзацы, примеры, в конце — упражнение.",
                    "Не давай сразу полное решение задачи, пока ученик не попробовал сам.",
                    "",
                ),
            ];
            for (name, description, role, guidelines, output_format, constraints, triggers) in seeded {
                conn.execute(
                    "INSERT OR IGNORE INTO skills
                     (id, name, description, role, guidelines, output_format, examples, constraints, triggers, enabled, builtin, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, 0, 1, ?9)",
                    params![
                        new_id(),
                        name,
                        description,
                        role,
                        guidelines,
                        output_format,
                        constraints,
                        triggers,
                        now()
                    ],
                )?;
            }
            conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])?;
        }

        if current < 3 {
            // Самообучение: усвоенные правила, оценки ответов и датасеты для дообучения.
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS lessons (
                  id TEXT PRIMARY KEY,
                  text TEXT NOT NULL,
                  source TEXT,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS feedback (
                  id TEXT PRIMARY KEY,
                  message_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  rating INTEGER NOT NULL,
                  note TEXT,
                  created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_feedback_message ON feedback(message_id);

                CREATE TABLE IF NOT EXISTS datasets (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  path TEXT NOT NULL,
                  teacher_model TEXT,
                  examples INTEGER NOT NULL DEFAULT 0,
                  created_at TEXT NOT NULL
                );
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (3)", [])?;
        }

        if current < 4 {
            // Источники интернет-поиска, привязанные к ответу модели.
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS message_sources (
                  id TEXT PRIMARY KEY,
                  message_id TEXT NOT NULL,
                  session_id TEXT NOT NULL,
                  title TEXT,
                  url TEXT NOT NULL,
                  snippet TEXT,
                  created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_sources_message ON message_sources(message_id);
                CREATE INDEX IF NOT EXISTS idx_sources_session ON message_sources(session_id);
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (4)", [])?;
        }

        if current < 5 {
            // Рабочая папка проекта для каждой сессии.
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN workdir TEXT;",
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (5)", [])?;
        }

        if current < 6 {
            // Режим работы, политика доверия и уровень рассуждений для сессии.
            conn.execute_batch(
                r#"
                ALTER TABLE sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'chat';
                ALTER TABLE sessions ADD COLUMN trust TEXT NOT NULL DEFAULT 'workspace';
                ALTER TABLE sessions ADD COLUMN reasoning TEXT;
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])?;
        }

        if current < 7 {
            // Уровни рассуждений, которые рекламирует провайдер (/models → effort.supported_levels).
            conn.execute_batch("ALTER TABLE provider_models ADD COLUMN efforts TEXT;")?;
            conn.execute("INSERT INTO schema_version (version) VALUES (7)", [])?;
        }

        if current < 8 {
            // История вызовов инструментов M6 (чтобы модель помнила, что уже делала).
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS tool_runs (
                  id TEXT PRIMARY KEY,
                  session_id TEXT NOT NULL,
                  request_id TEXT,
                  tool TEXT NOT NULL,
                  path TEXT,
                  ok INTEGER NOT NULL DEFAULT 1,
                  summary TEXT,
                  output TEXT,
                  created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_tool_runs_session ON tool_runs(session_id);
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (8)", [])?;
        }

        if current < 9 {
            // Резервные копии перед изменениями (для «Отменить последнее»).
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS tool_backups (
                  id TEXT PRIMARY KEY,
                  session_id TEXT NOT NULL,
                  request_id TEXT,
                  tool TEXT NOT NULL,
                  action TEXT NOT NULL,
                  path TEXT NOT NULL,
                  target TEXT,
                  backup_path TEXT,
                  created INTEGER NOT NULL DEFAULT 0,
                  undone INTEGER NOT NULL DEFAULT 0,
                  created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_tool_backups_session ON tool_backups(session_id);
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (9)", [])?;
        }

        if current < 10 {
            // Локальный RAG: чанки файлов рабочей папки и полнотекстовый индекс.
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS rag_chunks (
                  id TEXT PRIMARY KEY,
                  path TEXT NOT NULL,
                  chunk_index INTEGER NOT NULL,
                  content TEXT NOT NULL,
                  created_at TEXT NOT NULL
                );
                CREATE VIRTUAL TABLE IF NOT EXISTS rag_fts
                  USING fts5(chunk_id UNINDEXED, path UNINDEXED, content, tokenize='unicode61');
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (10)", [])?;
        }

        if current < 11 {
            // RAG привязывается к рабочей папке (root), чтобы проекты не перетирали индекс.
            conn.execute_batch(
                r#"
                ALTER TABLE rag_chunks ADD COLUMN root TEXT;
                CREATE INDEX IF NOT EXISTS idx_rag_root ON rag_chunks(root);
                DELETE FROM rag_fts;
                DELETE FROM rag_chunks;
                "#,
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (11)", [])?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ сессии
    pub fn sessions_list(&self) -> Result<Vec<Session>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, title, model_id, created_at, updated_at, workdir, mode, trust, reasoning
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    model_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    workdir: row.get(5)?,
                    mode: row.get(6)?,
                    trust: row.get(7)?,
                    reasoning: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn session_get(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, title, model_id, created_at, updated_at, workdir, mode, trust, reasoning
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Session {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        model_id: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        workdir: row.get(5)?,
                        mode: row.get(6)?,
                        trust: row.get(7)?,
                        reasoning: row.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn session_create(&self, title: &str, model_id: &str) -> Result<Session> {
        let session = Session {
            id: new_id(),
            title: title.to_string(),
            model_id: model_id.to_string(),
            created_at: now(),
            updated_at: now(),
            workdir: None,
            mode: "chat".to_string(),
            trust: "workspace".to_string(),
            reasoning: None,
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, model_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session.id, session.title, session.model_id, session.created_at, session.updated_at],
        )?;
        Ok(session)
    }

    pub fn session_set_workdir(&self, id: &str, workdir: Option<&str>) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET workdir = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, workdir, now()],
        )?;
        Ok(())
    }

    /// Обновляет режим/доверие/рассуждения сессии (None — не менять).
    pub fn session_set_policy(
        &self,
        id: &str,
        mode: Option<&str>,
        trust: Option<&str>,
        reasoning: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET
                mode = COALESCE(?2, mode),
                trust = COALESCE(?3, trust),
                reasoning = COALESCE(?4, reasoning),
                updated_at = ?5
             WHERE id = ?1",
            params![id, mode, trust, reasoning, now()],
        )?;
        Ok(())
    }

    /// Устанавливает или сбрасывает (None) уровень рассуждений.
    pub fn session_set_reasoning(&self, id: &str, reasoning: Option<&str>) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET reasoning = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, reasoning, now()],
        )?;
        Ok(())
    }

    pub fn session_rename(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now()],
        )?;
        Ok(())
    }

    pub fn session_set_model(&self, id: &str, model_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET model_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, model_id, now()],
        )?;
        Ok(())
    }

    pub fn session_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM message_sources WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM tool_runs WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM tool_backups WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn session_touch(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("UPDATE sessions SET updated_at = ?2 WHERE id = ?1", params![id, now()])?;
        Ok(())
    }

    // ------------------------------------------------------------------ сообщения
    pub fn messages_list(&self, session_id: &str) -> Result<Vec<ChatMessageRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, model_id, tokens, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(ChatMessageRow {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    model_id: row.get(4)?,
                    tokens: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Поиск по сообщениям и названиям сессий (для панели поиска).
    pub fn messages_search(&self, query: &str, limit: usize) -> Result<Vec<MessageHitRow>> {
        let raw = query.trim();
        if raw.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let escaped = raw
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.session_id, s.title, m.id, m.role,
                    CASE WHEN instr(lower(m.content), lower(?1)) > 0
                         THEN substr(m.content, max(1, instr(lower(m.content), lower(?1)) - 40), 220)
                         ELSE substr(m.content, 1, 220) END,
                    m.created_at
             FROM messages m JOIN sessions s ON s.id = m.session_id
             WHERE m.content LIKE ?2 ESCAPE '\\' OR s.title LIKE ?2 ESCAPE '\\'
             ORDER BY m.rowid DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![raw, pattern, limit as i64], |row| {
                Ok(MessageHitRow {
                    session_id: row.get(0)?,
                    session_title: row.get(1)?,
                    message_id: row.get(2)?,
                    role: row.get(3)?,
                    snippet: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn message_add(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        model_id: Option<&str>,
        tokens: Option<i64>,
    ) -> Result<ChatMessageRow> {
        let message = ChatMessageRow {
            id: new_id(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            model_id: model_id.map(|s| s.to_string()),
            tokens,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO messages (id, session_id, role, content, model_id, tokens, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message.id,
                message.session_id,
                message.role,
                message.content,
                message.model_id,
                message.tokens,
                message.created_at
            ],
        )?;
        Ok(message)
    }

    pub fn message_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Удаляет сообщение и все последующие в сессии. Возвращает id удалённых.
    pub fn messages_delete_from(&self, session_id: &str, id: &str) -> Result<Vec<String>> {
        let conn = self.conn();
        let rowid: Option<i64> = conn
            .query_row(
                "SELECT rowid FROM messages WHERE id = ?1 AND session_id = ?2",
                params![id, session_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(rowid) = rowid else {
            return Ok(vec![]);
        };
        let mut stmt = conn.prepare(
            "SELECT id FROM messages WHERE session_id = ?1 AND rowid >= ?2 ORDER BY rowid",
        )?;
        let ids = stmt
            .query_map(params![session_id, rowid], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND rowid >= ?2",
            params![session_id, rowid],
        )?;
        Ok(ids)
    }

    /// Удаляет последний ответ ассистента (для перегенерации).
    pub fn message_delete_last_assistant(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM messages WHERE session_id = ?1 AND role = 'assistant'
                 ORDER BY rowid DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(ref id) = id {
            conn.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        }
        Ok(id)
    }

    pub fn provider_model_get(&self, provider_id: &str, model_id: &str) -> Result<Option<ProviderModelRow>> {        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT provider_id, model_id, label, context, efforts, updated_at FROM provider_models
                 WHERE provider_id = ?1 AND model_id = ?2",
                params![provider_id, model_id],
                |row| {
                    Ok(ProviderModelRow {
                        provider_id: row.get(0)?,
                        model_id: row.get(1)?,
                        label: row.get(2)?,
                        context: row.get(3)?,
                        efforts: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    // ------------------------------------------------------------------ память
    pub fn memory_add(&self, kind: &str, text: &str, source_session: Option<&str>) -> Result<MemoryRow> {
        let row = MemoryRow {
            id: new_id(),
            kind: kind.to_string(),
            text: text.to_string(),
            source_session: source_session.map(|s| s.to_string()),
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO memories (id, kind, text, source_session, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![row.id, row.kind, row.text, row.source_session, row.created_at],
        )?;
        Ok(row)
    }

    pub fn memories_list(&self, limit: i64) -> Result<Vec<MemoryRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, kind, text, source_session, created_at FROM memories ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], map_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn memory_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// FTS5/BM25 по тексту памяти, топ-K фрагментов.
    pub fn memory_search(&self, query: &str, limit: i64) -> Result<Vec<MemoryRow>> {
        let match_query = fts_query(query);
        if match_query.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.kind, m.text, m.source_session, m.created_at
             FROM memories_fts f JOIN memories m ON m.rowid = f.rowid
             WHERE memories_fts MATCH ?1
             ORDER BY bm25(memories_fts) LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![match_query, limit], map_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        if !rows.is_empty() {
            return Ok(rows);
        }
        // Fallback: MATCH может не найти ничего при необычной морфологии — ищем по подстроке.
        let mut like_stmt = conn.prepare(
            "SELECT id, kind, text, source_session, created_at FROM memories
             WHERE text LIKE ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let pattern = format!("%{}%", query.trim());
        let rows = like_stmt
            .query_map(params![pattern, limit], map_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------------ провайдеры
    pub fn providers_list(&self) -> Result<Vec<ProviderRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, kind, label, base_url, key_ref, enabled, timeout_s, created_at
             FROM providers ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_provider)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn provider_get(&self, id: &str) -> Result<Option<ProviderRow>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, kind, label, base_url, key_ref, enabled, timeout_s, created_at
                 FROM providers WHERE id = ?1",
                params![id],
                map_provider,
            )
            .optional()?)
    }

    pub fn provider_upsert(&self, p: &ProviderRow) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO providers (id, kind, label, base_url, key_ref, enabled, timeout_s, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, label=excluded.label,
               base_url=excluded.base_url, key_ref=excluded.key_ref, enabled=excluded.enabled,
               timeout_s=excluded.timeout_s",
            params![
                p.id,
                p.kind,
                p.label,
                p.base_url,
                p.key_ref,
                p.enabled as i64,
                p.timeout_s,
                p.created_at
            ],
        )?;
        Ok(())
    }

    pub fn provider_set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE providers SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    pub fn provider_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn provider_models(&self, provider_id: &str) -> Result<Vec<ProviderModelRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT provider_id, model_id, label, context, efforts, updated_at FROM provider_models
             WHERE provider_id = ?1 ORDER BY model_id ASC",
        )?;
        let rows = stmt
            .query_map(params![provider_id], |row| {
                Ok(ProviderModelRow {
                    provider_id: row.get(0)?,
                    model_id: row.get(1)?,
                    label: row.get(2)?,
                    context: row.get(3)?,
                    efforts: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn provider_models_all(&self) -> Result<Vec<ProviderModelRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT provider_id, model_id, label, context, efforts, updated_at FROM provider_models
             ORDER BY provider_id, model_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProviderModelRow {
                    provider_id: row.get(0)?,
                    model_id: row.get(1)?,
                    label: row.get(2)?,
                    context: row.get(3)?,
                    efforts: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn provider_models_replace(
        &self,
        provider_id: &str,
        models: &[(String, Option<String>, Option<i64>, Option<String>)],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM provider_models WHERE provider_id = ?1", params![provider_id])?;
        let stamp = now();
        for (model_id, label, context, efforts) in models {
            tx.execute(
                "INSERT INTO provider_models (provider_id, model_id, label, context, efforts, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![provider_id, model_id, label, context, efforts, stamp],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Удаляет все сообщения сессии (команда /clear и сжатие истории).
    pub fn messages_delete_all(&self, session_id: &str) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM messages WHERE session_id = ?1 ORDER BY rowid")?;
        let ids = stmt
            .query_map(params![session_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session_id])?;
        Ok(ids)
    }

    // ------------------------------------------------------------------ инструменты
    pub fn tool_run_add(
        &self,
        session_id: &str,
        request_id: &str,
        tool: &str,
        path: Option<&str>,
        ok: bool,
        summary: &str,
        output: &str,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_runs
             (id, session_id, request_id, tool, path, ok, summary, output, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                new_id(),
                session_id,
                request_id,
                tool,
                path,
                ok as i64,
                summary,
                output,
                now()
            ],
        )?;
        Ok(())
    }

    pub fn tool_runs_recent(&self, session_id: &str, limit: usize) -> Result<Vec<ToolRunRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, request_id, tool, path, ok, summary, output, created_at
             FROM tool_runs WHERE session_id = ?1 ORDER BY rowid DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![session_id, limit as i64], |row| {
                Ok(ToolRunRow {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    request_id: row.get(2)?,
                    tool: row.get(3)?,
                    path: row.get(4)?,
                    ok: row.get::<_, i64>(5)? != 0,
                    summary: row.get(6)?,
                    output: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn tool_runs_delete(&self, session_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM tool_runs WHERE session_id = ?1", params![session_id])?;
        Ok(())
    }

    // ------------------------------------------------------------------ резервные копии
    #[allow(clippy::too_many_arguments)]
    pub fn tool_backup_add(
        &self,
        session_id: &str,
        request_id: &str,
        tool: &str,
        action: &str,
        path: &str,
        target: Option<&str>,
        backup_path: Option<&str>,
        created: bool,
    ) -> Result<ToolBackupRow> {
        let row = ToolBackupRow {
            id: new_id(),
            session_id: session_id.to_string(),
            request_id: Some(request_id.to_string()),
            tool: tool.to_string(),
            action: action.to_string(),
            path: path.to_string(),
            target: target.map(|value| value.to_string()),
            backup_path: backup_path.map(|value| value.to_string()),
            created,
            undone: false,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_backups
             (id, session_id, request_id, tool, action, path, target, backup_path, created, undone, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)",
            params![
                row.id,
                row.session_id,
                row.request_id,
                row.tool,
                row.action,
                row.path,
                row.target,
                row.backup_path,
                row.created as i64,
                row.created_at
            ],
        )?;
        Ok(row)
    }

    pub fn tool_backup_last(&self, session_id: &str) -> Result<Option<ToolBackupRow>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, session_id, request_id, tool, action, path, target, backup_path,
                        created, undone, created_at
                 FROM tool_backups WHERE session_id = ?1 AND undone = 0
                 ORDER BY rowid DESC LIMIT 1",
                params![session_id],
                |row| {
                    Ok(ToolBackupRow {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        request_id: row.get(2)?,
                        tool: row.get(3)?,
                        action: row.get(4)?,
                        path: row.get(5)?,
                        target: row.get(6)?,
                        backup_path: row.get(7)?,
                        created: row.get::<_, i64>(8)? != 0,
                        undone: row.get::<_, i64>(9)? != 0,
                        created_at: row.get(10)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn tool_backup_mark_undone(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE tool_backups SET undone = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ RAG
    pub fn rag_clear(&self, root: &str) -> Result<()> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM rag_chunks WHERE root = ?1")?;
        let ids = stmt
            .query_map(params![root], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for id in ids {
            conn.execute("DELETE FROM rag_fts WHERE chunk_id = ?1", params![id])?;
        }
        conn.execute("DELETE FROM rag_chunks WHERE root = ?1", params![root])?;
        Ok(())
    }

    pub fn rag_add_chunk(
        &self,
        root: &str,
        path: &str,
        chunk_index: i64,
        content: &str,
    ) -> Result<()> {
        let conn = self.conn();
        let id = new_id();
        conn.execute(
            "INSERT INTO rag_chunks (id, root, path, chunk_index, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, root, path, chunk_index, content, now()],
        )?;
        conn.execute(
            "INSERT INTO rag_fts (chunk_id, path, content) VALUES (?1, ?2, ?3)",
            params![id, path, content],
        )?;
        Ok(())
    }

    pub fn rag_search(&self, root: &str, query: &str, limit: usize) -> Result<Vec<RagHitRow>> {
        let fts = fts_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.path, c.content
             FROM rag_fts f JOIN rag_chunks c ON c.id = f.chunk_id
             WHERE c.root = ?1 AND rag_fts MATCH ?2
             ORDER BY bm25(rag_fts)
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![root, fts, limit as i64], |row| {
                Ok(RagHitRow {
                    path: row.get(0)?,
                    content: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rag_stats(&self, root: &str) -> Result<RagStats> {
        let conn = self.conn();
        let chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rag_chunks WHERE root = ?1",
            params![root],
            |row| row.get(0),
        )?;
        let files: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM rag_chunks WHERE root = ?1",
            params![root],
            |row| row.get(0),
        )?;
        Ok(RagStats { chunks, files })
    }

    /// Убирает из индекса все чанки одного файла.
    pub fn rag_delete_path(&self, root: &str, path: &str) -> Result<()> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id FROM rag_chunks WHERE root = ?1 AND path = ?2")?;
        let ids = stmt
            .query_map(params![root, path], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for id in ids {
            conn.execute("DELETE FROM rag_fts WHERE chunk_id = ?1", params![id])?;
        }
        conn.execute(
            "DELETE FROM rag_chunks WHERE root = ?1 AND path = ?2",
            params![root, path],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ навыки
    pub fn skills_list(&self) -> Result<Vec<SkillRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, role, guidelines, output_format, examples, constraints,
                    triggers, enabled, builtin, created_at
             FROM skills ORDER BY builtin DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], map_skill)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn skill_get(&self, id: &str) -> Result<Option<SkillRow>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, name, description, role, guidelines, output_format, examples, constraints,
                        triggers, enabled, builtin, created_at
                 FROM skills WHERE id = ?1",
                params![id],
                map_skill,
            )
            .optional()?)
    }

    pub fn skill_get_by_name(&self, name: &str) -> Result<Option<SkillRow>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, name, description, role, guidelines, output_format, examples, constraints,
                        triggers, enabled, builtin, created_at
                 FROM skills WHERE name = ?1 COLLATE NOCASE",
                params![name.trim()],
                map_skill,
            )
            .optional()?)
    }

    pub fn skill_save(&self, row: &SkillRow) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO skills (id, name, description, role, guidelines, output_format, examples,
                                 constraints, triggers, enabled, builtin, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               description = excluded.description,
               role = excluded.role,
               guidelines = excluded.guidelines,
               output_format = excluded.output_format,
               examples = excluded.examples,
               constraints = excluded.constraints,
               triggers = excluded.triggers,
               enabled = excluded.enabled",
            params![
                row.id,
                row.name,
                row.description,
                row.role,
                row.guidelines,
                row.output_format,
                row.examples,
                row.constraints,
                row.triggers,
                row.enabled as i64,
                row.builtin as i64,
                row.created_at
            ],
        )?;
        Ok(())
    }

    pub fn skill_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM skills WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn skill_set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE skills SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ обучение
    pub fn lessons_list(&self) -> Result<Vec<LessonRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, text, source, enabled, created_at FROM lessons ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], map_lesson)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn lesson_add(&self, text: &str, source: Option<&str>) -> Result<LessonRow> {
        let row = LessonRow {
            id: new_id(),
            text: text.trim().to_string(),
            source: source.map(|s| s.to_string()),
            enabled: true,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO lessons (id, text, source, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![row.id, row.text, row.source, row.enabled as i64, row.created_at],
        )?;
        Ok(row)
    }

    pub fn lesson_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM lessons WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn lesson_set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE lessons SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    /// Одна оценка на сообщение: новая перезаписывает старую.
    pub fn feedback_set(
        &self,
        message_id: &str,
        session_id: &str,
        rating: i64,
        note: Option<&str>,
    ) -> Result<FeedbackRow> {
        let row = FeedbackRow {
            id: new_id(),
            message_id: message_id.to_string(),
            session_id: session_id.to_string(),
            rating,
            note: note.map(|s| s.to_string()),
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute("DELETE FROM feedback WHERE message_id = ?1", params![message_id])?;
        conn.execute(
            "INSERT INTO feedback (id, message_id, session_id, rating, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![row.id, row.message_id, row.session_id, row.rating, row.note, row.created_at],
        )?;
        Ok(row)
    }

    pub fn feedback_list(&self, session_id: &str) -> Result<Vec<FeedbackRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, message_id, session_id, rating, note, created_at
             FROM feedback WHERE session_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(FeedbackRow {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    session_id: row.get(2)?,
                    rating: row.get(3)?,
                    note: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn dataset_add(
        &self,
        name: &str,
        path: &str,
        teacher_model: Option<&str>,
        examples: i64,
    ) -> Result<DatasetRow> {
        let row = DatasetRow {
            id: new_id(),
            name: name.trim().to_string(),
            path: path.to_string(),
            teacher_model: teacher_model.map(|s| s.to_string()),
            examples,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO datasets (id, name, path, teacher_model, examples, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![row.id, row.name, row.path, row.teacher_model, row.examples, row.created_at],
        )?;
        Ok(row)
    }

    pub fn datasets_list(&self) -> Result<Vec<DatasetRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, teacher_model, examples, created_at
             FROM datasets ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DatasetRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    teacher_model: row.get(3)?,
                    examples: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn dataset_delete(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM datasets WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn dataset_set_examples(&self, id: &str, examples: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE datasets SET examples = ?2 WHERE id = ?1",
            params![id, examples],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ источники
    pub fn source_add(
        &self,
        session_id: &str,
        message_id: &str,
        title: &str,
        url: &str,
        snippet: &str,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO message_sources (id, message_id, session_id, title, url, snippet, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), message_id, session_id, title, url, snippet, now()],
        )?;
        Ok(())
    }

    pub fn sources_list(&self, session_id: &str) -> Result<Vec<SourceRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, message_id, session_id, title, url, snippet, created_at
             FROM message_sources WHERE session_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(SourceRow {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    session_id: row.get(2)?,
                    title: row.get(3)?,
                    url: row.get(4)?,
                    snippet: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------------ дебаты
    pub fn debate_create(&self, task: &str, participants: &str, rounds: i64, judge: Option<&str>) -> Result<DebateRow> {
        let row = DebateRow {
            id: new_id(),
            task: task.to_string(),
            participants: participants.to_string(),
            rounds,
            judge: judge.map(|s| s.to_string()),
            verdict: None,
            final_answer: None,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO debates (id, task, participants, rounds, judge, verdict, final_answer, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![row.id, row.task, row.participants, row.rounds, row.judge, row.verdict, row.final_answer, row.created_at],
        )?;
        Ok(row)
    }

    pub fn debate_finish(&self, id: &str, verdict: &str, final_answer: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE debates SET verdict = ?2, final_answer = ?3 WHERE id = ?1",
            params![id, verdict, final_answer],
        )?;
        Ok(())
    }

    pub fn debate_get(&self, id: &str) -> Result<Option<DebateRow>> {
        let conn = self.conn();
        Ok(conn
            .query_row(
                "SELECT id, task, participants, rounds, judge, verdict, final_answer, created_at
                 FROM debates WHERE id = ?1",
                params![id],
                |row| {
                    Ok(DebateRow {
                        id: row.get(0)?,
                        task: row.get(1)?,
                        participants: row.get(2)?,
                        rounds: row.get(3)?,
                        judge: row.get(4)?,
                        verdict: row.get(5)?,
                        final_answer: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn debates_list(&self, limit: i64) -> Result<Vec<DebateRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, task, participants, rounds, judge, verdict, final_answer, created_at
             FROM debates ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(DebateRow {
                    id: row.get(0)?,
                    task: row.get(1)?,
                    participants: row.get(2)?,
                    rounds: row.get(3)?,
                    judge: row.get(4)?,
                    verdict: row.get(5)?,
                    final_answer: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn debate_turn_add(
        &self,
        debate_id: &str,
        round: i64,
        role: &str,
        model_id: &str,
        content: &str,
        tokens: Option<i64>,
        seconds: Option<f64>,
    ) -> Result<DebateTurnRow> {
        let row = DebateTurnRow {
            id: new_id(),
            debate_id: debate_id.to_string(),
            round,
            role: role.to_string(),
            model_id: model_id.to_string(),
            content: content.to_string(),
            tokens,
            seconds,
            created_at: now(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO debate_turns (id, debate_id, round, role, model_id, content, tokens, seconds, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.debate_id,
                row.round,
                row.role,
                row.model_id,
                row.content,
                row.tokens,
                row.seconds,
                row.created_at
            ],
        )?;
        Ok(row)
    }

    pub fn debate_turns(&self, debate_id: &str) -> Result<Vec<DebateTurnRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, debate_id, round, role, model_id, content, tokens, seconds, created_at
             FROM debate_turns WHERE debate_id = ?1 ORDER BY round ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![debate_id], |row| {
                Ok(DebateTurnRow {
                    id: row.get(0)?,
                    debate_id: row.get(1)?,
                    round: row.get(2)?,
                    role: row.get(3)?,
                    model_id: row.get(4)?,
                    content: row.get(5)?,
                    tokens: row.get(6)?,
                    seconds: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------------ настройки
    pub fn setting_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn();
        Ok(conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .optional()?)
    }

    pub fn setting_remove(&self, key: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn map_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        text: row.get(2)?,
        source_session: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn map_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderRow> {
    Ok(ProviderRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        label: row.get(2)?,
        base_url: row.get(3)?,
        key_ref: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        timeout_s: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn map_lesson(row: &rusqlite::Row<'_>) -> rusqlite::Result<LessonRow> {
    Ok(LessonRow {
        id: row.get(0)?,
        text: row.get(1)?,
        source: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
    })
}

fn map_skill(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillRow> {
    Ok(SkillRow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        role: row.get(3)?,
        guidelines: row.get(4)?,
        output_format: row.get(5)?,
        examples: row.get(6)?,
        constraints: row.get(7)?,
        triggers: row.get(8)?,
        enabled: row.get::<_, i64>(9)? != 0,
        builtin: row.get::<_, i64>(10)? != 0,
        created_at: row.get(11)?,
    })
}

/// Строит безопасный FTS5-запрос из свободного текста: токены через OR с префиксом.
pub fn fts_query(query: &str) -> String {
    let mut tokens: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| t.chars().count() >= 3)
        .map(|t| t.to_lowercase())
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens.truncate(12);
    tokens
        .into_iter()
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_and_messages_roundtrip() {
        let db = Db::open_memory().unwrap();
        let s = db.session_create("Новая сессия", "local/qwen-1.5b").unwrap();
        db.message_add(&s.id, "user", "привет", None, None).unwrap();
        db.message_add(&s.id, "assistant", "здравствуйте", Some("local/qwen-1.5b"), Some(5)).unwrap();
        let messages = db.messages_list(&s.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].tokens, Some(5));
        db.session_rename(&s.id, "Тема").unwrap();
        assert_eq!(db.session_get(&s.id).unwrap().unwrap().title, "Тема");
        db.session_delete(&s.id).unwrap();
        assert!(db.messages_list(&s.id).unwrap().is_empty());
    }

    #[test]
    fn backup_and_restore_database() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("db-backup-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.db");
        let db = Db::open(&path).unwrap();
        let first = db.session_create("Первая", "local/qwen-1.5b").unwrap();
        let backup = dir.join("copy.db");
        db.backup_to(&backup).unwrap();
        assert!(backup.is_file());

        let second = db.session_create("Вторая", "local/qwen-1.5b").unwrap();
        assert_eq!(db.sessions_list().unwrap().len(), 2);

        db.restore_from(&backup).unwrap();
        let sessions = db.sessions_list().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, first.id);
        assert!(db.session_get(&second.id).unwrap().is_none());

        assert!(Db::open(&path).unwrap().sessions_list().unwrap().len() == 1);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_delete_cleans_children() {
        let db = Db::open_memory().unwrap();
        let s = db.session_create("Удаляемая", "local/qwen-1.5b").unwrap();
        db.message_add(&s.id, "user", "hi", None, None).unwrap();
        db.tool_run_add(&s.id, "r1", "read_file", Some("a.md"), true, "ok", "out")
            .unwrap();
        db.tool_backup_add(
            &s.id,
            "r1",
            "write_file",
            "write",
            "C:\\tmp\\a.md",
            None,
            None,
            true,
        )
        .unwrap();
        db.session_delete(&s.id).unwrap();
        assert!(db.session_get(&s.id).unwrap().is_none());
        assert!(db.messages_list(&s.id).unwrap().is_empty());
        assert!(db.tool_runs_recent(&s.id, 10).unwrap().is_empty());
        assert!(db.tool_backup_last(&s.id).unwrap().is_none());
    }

    #[test]
    fn messages_search_finds_and_escapes() {
        let db = Db::open_memory().unwrap();
        let s = db.session_create("Прошивка", "local/qwen-1.5b").unwrap();
        db.message_add(&s.id, "user", "в binres используется JFFS2", None, None)
            .unwrap();
        let hits = db.messages_search("JFFS2", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_title, "Прошивка");
        assert!(hits[0].snippet.contains("JFFS2"));
        assert!(db.messages_search("%", 10).unwrap().is_empty());
        assert!(db.messages_search("a", 10).unwrap().is_empty());
    }

    #[test]
    fn memory_search_uses_fts() {
        let db = Db::open_memory().unwrap();
        db.memory_add("fact", "в binres используется JFFS2", None).unwrap();
        db.memory_add("preference", "отвечай кратко", None).unwrap();
        let found = db.memory_search("какой формат используется в binres", 5).unwrap();
        assert!(found.iter().any(|m| m.text.contains("JFFS2")));
        assert!(!db.memory_search("совершенно иное", 5).unwrap().iter().any(|m| m.text.contains("JFFS2")));
    }

    #[test]
    fn debate_turns_saved() {
        let db = Db::open_memory().unwrap();
        let d = db.debate_create("задача", r#"["local/coder-7b","local/qwen3-14b"]"#, 2, None).unwrap();
        db.debate_turn_add(&d.id, 1, "proposal", "local/coder-7b", "текст", Some(10), Some(1.5))
            .unwrap();
        db.debate_finish(&d.id, "вердикт", "ответ").unwrap();
        let turns = db.debate_turns(&d.id).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, "proposal");
        assert_eq!(db.debate_get(&d.id).unwrap().unwrap().final_answer.as_deref(), Some("ответ"));
    }

    #[test]
    fn fts_query_is_sanitized() {
        assert_eq!(fts_query("!!!"), "");
        assert!(fts_query("Формат дампа").contains("формат"));
        assert!(!fts_query("a b").contains("a\""));
    }
}
