"""Реестр локальных моделей OpenVINO IR.

Модели — это папки с openvino_model.xml/.bin и токенизаторами, а не GGUF.
Пути фиксированы заданием; конфиг может их переопределить (--models-config).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

# Значения по умолчанию из TASK.md, раздел 5.2.
MODELS: dict[str, dict[str, Any]] = {
    "qwen3-14b": {
        "path": r"C:\Models\qwen3-14b-int4-ov",
        "label": "Qwen3-14B",
        "device": "GPU",
        "thinks": True,
        "size_gb": 9.06,
        "max_tokens": 2048,
    },
    "coder-30b": {
        "path": r"C:\Models\qwen3-coder-30b-a3b-int4-ov",
        "label": "Qwen3-Coder-30B-A3B",
        "device": "GPU",
        "thinks": False,
        "size_gb": 15.2,
        "max_tokens": 2048,
    },
    "coder-7b": {
        "path": r"C:\Models\qwen2.5-coder-7b-int4-ov",
        "label": "Qwen2.5-Coder-7B",
        "device": "GPU",
        "thinks": False,
        "size_gb": 4.2,
        "max_tokens": 2048,
    },
    "qwen-1.5b": {
        "path": r"C:\Models\qwen2.5-1.5b-int4-ov",
        "label": "Qwen2.5-1.5B",
        "device": "NPU",
        "thinks": False,
        "size_gb": 0.87,
        "max_tokens": 1024,
    },
}

DEFAULT_CACHE_DIR = r"C:\Models\cache"
# Модели крупнее этого порога нельзя держать в памяти одновременно.
HEAVY_THRESHOLD_GB = 8.0


class Registry:
    """Список моделей + каталог кэша компиляции OpenVINO."""

    def __init__(self, models: dict[str, dict[str, Any]] | None = None, cache_dir: str | None = None):
        self.models = models or MODELS
        self.cache_dir = cache_dir or DEFAULT_CACHE_DIR

    def get(self, model_id: str) -> dict[str, Any] | None:
        return self.models.get(model_id)

    def ids(self) -> list[str]:
        return list(self.models)

    def as_list(self, loaded: set[str]) -> list[dict[str, Any]]:
        return [
            {
                "id": model_id,
                "label": cfg["label"],
                "device": cfg["device"],
                "size_gb": cfg["size_gb"],
                "thinks": cfg.get("thinks", False),
                "max_tokens": cfg.get("max_tokens", 2048),
                "loaded": model_id in loaded,
            }
            for model_id, cfg in self.models.items()
        ]

    @classmethod
    def from_config_file(cls, path: str | os.PathLike[str] | None) -> "Registry":
        """Читает JSON {"models": {...}, "cache_dir": "..."}; при ошибке — значения по умолчанию."""
        if not path:
            return cls()
        try:
            data = json.loads(Path(path).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return cls()
        models = data.get("models") or MODELS
        return cls(models=models, cache_dir=data.get("cache_dir") or DEFAULT_CACHE_DIR)
