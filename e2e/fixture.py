"""Подготовка и проверки для E2E-прогона Legamind."""
from __future__ import annotations

import datetime as dt
import json
import os
import shutil
import sqlite3
import sys
import uuid
from pathlib import Path

DB = Path(os.path.expandvars(r"%APPDATA%\com.legamind.app\legamind.db"))
TITLE = "E2E smoke"


def connect() -> sqlite3.Connection:
    conn = sqlite3.connect(DB)
    return conn


def setup(project: Path) -> dict:
    shutil.rmtree(project, ignore_errors=True)
    project.mkdir(parents=True, exist_ok=True)
    (project / "notes.md").write_text(
        "# E2E\n\nУникальный токен проекта: E2E_RAG_TOKEN_246.\n", encoding="utf-8"
    )
    conn = connect()
    old = [row[0] for row in conn.execute("SELECT id FROM sessions WHERE title = ?", (TITLE,))]
    for session_id in old:
        conn.execute(
            "DELETE FROM message_sources WHERE message_id IN (SELECT id FROM messages WHERE session_id = ?)",
            (session_id,),
        )
        conn.execute("DELETE FROM messages WHERE session_id = ?", (session_id,))
        conn.execute("DELETE FROM tool_runs WHERE session_id = ?", (session_id,))
        conn.execute("DELETE FROM sessions WHERE id = ?", (session_id,))
    conn.execute("UPDATE sessions SET pinned = 0")
    session_id = str(uuid.uuid4())
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    conn.execute(
        """INSERT INTO sessions
           (id, title, model_id, created_at, updated_at, workdir, mode, trust, reasoning, pinned)
           VALUES (?, ?, ?, ?, ?, ?, 'chat', 'workspace', NULL, 1)""",
        (session_id, TITLE, "deepseek/deepseek-flash", now, now, str(project)),
    )
    conn.execute("DELETE FROM session_permissions")
    conn.commit()
    conn.close()
    return {"session_id": session_id, "project": str(project)}


def facts() -> dict:
    conn = connect()
    session = conn.execute(
        "SELECT id FROM sessions WHERE title = ? ORDER BY updated_at DESC LIMIT 1", (TITLE,)
    ).fetchone()
    session_id = session[0] if session else None
    result = {
        "session_id": session_id,
        "permissions": [
            row[0] for row in conn.execute("SELECT kind FROM session_permissions")
        ],
        "messages": 0,
        "assistant": None,
        "last_tool": None,
        "tool_message_linked": False,
        "chunks": 0,
        "rag_token_chunks": 0,
        "input_tokens": 0,
        "output_tokens": 0,
    }
    if session_id:
        result["messages"] = conn.execute(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?", (session_id,)
        ).fetchone()[0]
        row = conn.execute(
            "SELECT content FROM messages WHERE session_id = ? AND role = 'assistant' ORDER BY rowid DESC LIMIT 1",
            (session_id,),
        ).fetchone()
        result["assistant"] = row[0][:400] if row else None
        usage = conn.execute(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(tokens), 0) FROM messages WHERE session_id = ? AND role = 'assistant'",
            (session_id,),
        ).fetchone()
        result["input_tokens"], result["output_tokens"] = usage
        tool = conn.execute(
            "SELECT tool, path, message_id FROM tool_runs WHERE session_id = ? ORDER BY rowid DESC LIMIT 1",
            (session_id,),
        ).fetchone()
        if tool:
            result["last_tool"] = {"tool": tool[0], "path": tool[1]}
            result["tool_message_linked"] = bool(tool[2])
        result["chunks"] = conn.execute(
            "SELECT COUNT(*) FROM rag_chunks WHERE root LIKE ?", (f"%e2e-project%",)
        ).fetchone()[0]
        result["rag_token_chunks"] = conn.execute(
            "SELECT COUNT(*) FROM rag_chunks WHERE content LIKE '%E2E_RAG_TOKEN_246%'"
        ).fetchone()[0]
    conn.close()
    return result


def cleanup() -> dict:
    conn = connect()
    old = [row[0] for row in conn.execute("SELECT id FROM sessions WHERE title = ?", (TITLE,))]
    for session_id in old:
        conn.execute(
            "DELETE FROM message_sources WHERE message_id IN (SELECT id FROM messages WHERE session_id = ?)",
            (session_id,),
        )
        conn.execute("DELETE FROM messages WHERE session_id = ?", (session_id,))
        conn.execute("DELETE FROM tool_runs WHERE session_id = ?", (session_id,))
        conn.execute("DELETE FROM sessions WHERE id = ?", (session_id,))
    conn.execute("DELETE FROM session_permissions")
    conn.commit()
    conn.close()
    return {"removed": len(old)}


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "setup":
        print(json.dumps(setup(Path(sys.argv[2])), ensure_ascii=False))
    elif mode == "facts":
        print(json.dumps(facts(), ensure_ascii=False))
    elif mode == "cleanup":
        print(json.dumps(cleanup(), ensure_ascii=False))
    else:
        raise SystemExit(f"unknown mode {mode}")
