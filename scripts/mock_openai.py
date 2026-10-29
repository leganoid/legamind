#!/usr/bin/env python
"""Мок OpenAI-совместимого сервера для приёмки M2.

Запуск: python scripts/mock_openai.py [--port 8099]
Отдаёт GET /v1/models и POST /v1/chat/completions (SSE, ~3 токена в секунду).
Ключ не проверяется — подходит любой.
"""

from __future__ import annotations

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODELS = [
    {"id": "mock-fast", "object": "model", "owned_by": "mock", "context_length": 8192},
    {"id": "mock-smart", "object": "model", "owned_by": "mock", "context_length": 32768},
]

ANSWER = (
    "Это ответ мок-сервера. Он стримится по одному токену, чтобы проверить "
    "поток delta/done в приложении. Ключ не проверяется, база — 127.0.0.1."
).split(" ")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt: str, *args) -> None:  # тише в консоли
        print(f"[mock] {fmt % args}", flush=True)

    def _json(self, payload: dict, status: int = 200) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path.rstrip("/") in ("/v1/models", "/models"):
            self._json({"object": "list", "data": MODELS})
        else:
            self._json({"error": {"message": "not found"}}, 404)

    def do_POST(self) -> None:
        if self.path.rstrip("/") not in ("/v1/chat/completions", "/chat/completions"):
            self._json({"error": {"message": "not found"}}, 404)
            return

        length = int(self.headers.get("Content-Length") or 0)
        try:
            request = json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            self._json({"error": {"message": "bad json"}}, 400)
            return

        model = request.get("model", "mock-fast")
        # Проверка ключа: мок принимает всё, но пустой заголовок — 401 (удобно для тестов).
        if "authorization" not in {k.lower() for k in self.headers.keys()}:
            self._json({"error": {"message": "missing api key"}}, 401)
            return

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()

        prompt_words = " ".join(
            message.get("content", "") for message in request.get("messages", [])
        ).split()
        prefix = response_prefix(prompt_words, model)

        try:
            for word in prefix:
                chunk = {
                    "id": "chatcmpl-mock",
                    "object": "chat.completion.chunk",
                    "model": model,
                    "choices": [{"index": 0, "delta": {"content": word + " "}, "finish_reason": None}],
                }
                self.wfile.write(f"data: {json.dumps(chunk, ensure_ascii=False)}\n\n".encode("utf-8"))
                self.wfile.flush()
                time.sleep(0.33)
            done = {
                "id": "chatcmpl-mock",
                "object": "chat.completion.chunk",
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": len(prompt_words), "completion_tokens": len(prefix)},
            }
            self.wfile.write(f"data: {json.dumps(done, ensure_ascii=False)}\n\n".encode("utf-8"))
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            return


def response_prefix(prompt_words: list[str], model: str) -> list[str]:
    question = " ".join(prompt_words[-12:])
    return [
        f"[{model}]",
        "Получен",
        "запрос:",
        f"«{question}».",
        *ANSWER,
    ]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8099)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"mock OpenAI server on http://127.0.0.1:{args.port}/v1", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.shutdown()


if __name__ == "__main__":
    main()
