"""Реестр локальных моделей OpenVINO IR.

Модели — это папки с openvino_model.xml/.bin и токенизаторами, а не GGUF.
Пути фиксированы заданием; конфиг может их переопределить (--models-config).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

# Пользователь добавляет локальные модели сам (Настройки → Модели).
# Встроенных путей нет: на чужой машине каталога C:\Models может не быть.
MODELS: dict[str, dict[str, Any]] = {}

DEFAULT_CACHE_DIR = str(Path.home() / ".legamind" / "cache")
# Модели крупнее этого порога нельзя держать в памяти одновременно.
HEAVY_THRESHOLD_GB = 8.0


class Registry:
    """Список моделей + каталог кэша компиляции OpenVINO."""

    def __init__(self, models: dict[str, dict[str, Any]] | None = None, cache_dir: str | None = None):
        self.models = models if models is not None else MODELS
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
