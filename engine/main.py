"""FastAPI-сайдкар legamind-engine.

Запуск: python -m engine --port 0 --models-config <path>
Печатает в stdout: HIVESIDECAR READY port=<port> pid=<pid>
Сеть — только 127.0.0.1.
"""

from __future__ import annotations

import asyncio
import json
import os
import threading
import time
from typing import Any

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse
from pydantic import BaseModel, Field

from .manager import ModelError, ModelManager
from .prompts import apply_think, with_default_system
from .registry import Registry


class Message(BaseModel):
    role: str
    content: str


class ChatRequest(BaseModel):
    model: str
    messages: list[Message]
    max_tokens: int = 1024
    temperature: float = 0.7
    think: bool = False


class ModelIdRequest(BaseModel):
    id: str


class EmbeddingsRequest(BaseModel):
    model: str
    texts: list[str] = Field(default_factory=list)


def create_app(registry: Registry) -> FastAPI:
    app = FastAPI(title="legamind-engine", docs_url=None, redoc_url=None)
    manager = ModelManager(registry)

    @app.exception_handler(ModelError)
    async def _model_error(_: Request, exc: ModelError) -> JSONResponse:
        return JSONResponse(status_code=400, content={"error": str(exc)})

    @app.get("/health")
    async def health() -> dict[str, Any]:
        return {"status": "ok", "models_loaded": manager.loaded_ids()}

    @app.get("/models")
    async def models() -> list[dict[str, Any]]:
        return registry.as_list(set(manager.loaded_ids()))

    @app.post("/models/load")
    async def models_load(body: ModelIdRequest) -> dict[str, Any]:
        return await asyncio.to_thread(manager.load, body.id)

    @app.post("/models/unload")
    async def models_unload(body: ModelIdRequest) -> dict[str, Any]:
        return manager.unload(body.id)

    @app.post("/chat")
    async def chat(body: ChatRequest, request: Request):
        print(f"[chat] request model={body.model}", flush=True)
        cfg = registry.get(body.model)
        if cfg is None:
            return JSONResponse(status_code=400, content={"error": f"Неизвестная модель: {body.model}"})

        messages = with_default_system([m.model_dump() for m in body.messages])
        last_user = next((m["content"] for m in reversed(messages) if m["role"] == "user"), "")
        if last_user:
            messages[-1]["content"] = apply_think(last_user, bool(cfg.get("thinks")), body.think)
        print("[chat] messages ready", flush=True)

        max_tokens = min(body.max_tokens or cfg.get("max_tokens", 1024), int(cfg.get("max_tokens", 2048)))

        # Загрузка (компиляция) — в отдельном потоке, чтобы не блокировать event loop.
        # Ошибки RAM/устройства превращаются в HTTP 400 с понятным текстом.
        try:
            await asyncio.to_thread(manager.ensure_loaded, body.model)
            print("[chat] model ready", flush=True)
        except ModelError as exc:
            print(f"[chat] load error: {exc}", flush=True)
            return JSONResponse(status_code=400, content={"error": str(exc)})

        cancelled = threading.Event()

        async def watch_disconnect() -> None:
            while not cancelled.is_set():
                if await request.is_disconnected():
                    cancelled.set()
                    return
                await asyncio.sleep(0.3)

        watcher = asyncio.create_task(watch_disconnect())

        def event_stream():
            print("[chat] stream start", flush=True)
            try:
                for event in manager.generate_stream(
                    body.model,
                    messages,
                    max_tokens,
                    body.temperature,
                    cancel_check=cancelled.is_set,
                ):
                    kind = event[0]
                    if kind == "delta":
                        payload = {"delta": event[1]}
                    elif kind == "done":
                        payload = {"done": True, "tokens": event[1], "seconds": event[2]}
                    else:  # pragma: no cover
                        continue
                    yield f"data: {json.dumps(payload, ensure_ascii=False)}\n\n"
                    if kind == "done":
                        break
            except ModelError as exc:
                yield f"data: {json.dumps({'error': str(exc)}, ensure_ascii=False)}\n\n"
            finally:
                cancelled.set()
                watcher.cancel()

        return StreamingResponse(
            event_stream(),
            media_type="text/event-stream",
            headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
        )

    @app.post("/embeddings")
    async def embeddings(body: EmbeddingsRequest):
        # В MVP векторы не используются: память работает на FTS5.
        return JSONResponse(
            status_code=400,
            content={"error": "Эмбеддинги не поддерживаются в этой версии (память использует FTS5)"},
        )

    return app
