"""Точка входа сайдкара: python -m engine --port 0 --models-config <path>."""

from __future__ import annotations

import argparse
import os
import socket
import sys
import threading
import time

import uvicorn

from .main import create_app
from .registry import Registry


def _pick_port(preferred: int) -> int:
    """Порт выбирает ОС (--port 0), Rust читает фактический из строки READY."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", preferred))
    port = sock.getsockname()[1]
    sock.close()
    return port


def _wait_listening(port: int, timeout: float = 60.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.05)
    return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="legamind-engine")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--models-config", default=None)
    args = parser.parse_args(argv)

    registry = Registry.from_config_file(args.models_config)
    app = create_app(registry)

    port = _pick_port(args.port)
    config = uvicorn.Config(app, host=args.host, port=port, log_level="warning", access_log=False)
    server = uvicorn.Server(config)

    thread = threading.Thread(target=server.run, name="uvicorn", daemon=True)
    thread.start()

    if not _wait_listening(port):
        print("LEGAMINDSIDECAR ERROR port bind timeout", flush=True)
        return 1

    print(f"LEGAMINDSIDECAR READY port={port} pid={os.getpid()}", flush=True)
    try:
        while thread.is_alive():
            thread.join(timeout=0.5)
    except KeyboardInterrupt:
        server.should_exit = True
    return 0


if __name__ == "__main__":
    sys.exit(main())
