"""Менеджер моделей: ленивая загрузка, LRU, сериализация доступа.

Правила из TASK.md 5.3:
* не более одной модели с size_gb > 8 в памяти;
* при нехватке — выгружать самую давно использованную;
* один LLMPipeline на модель, доступ под threading.Lock (OpenVINO не любит параллель);
* проверка свободной RAM перед загрузкой.
"""

from __future__ import annotations

import ctypes
import math
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Iterator

import openvino_genai as ov_genai

from .registry import HEAVY_THRESHOLD_GB, Registry
from .prompts import render_chatml

# NPU принимает только модели <= 1.5B (TASK.md, раздел 2).
NPU_MAX_GB = 2.0


class ModelError(RuntimeError):
    """Понятная пользователю ошибка загрузки/генерации."""


class _MemoryStatus(ctypes.Structure):
    _fields_ = [
        ("dwLength", ctypes.c_ulong),
        ("dwMemoryLoad", ctypes.c_ulong),
        ("ullTotalPhys", ctypes.c_ulonglong),
        ("ullAvailPhys", ctypes.c_ulonglong),
        ("ullTotalPageFile", ctypes.c_ulonglong),
        ("ullAvailPageFile", ctypes.c_ulonglong),
        ("ullTotalVirtual", ctypes.c_ulonglong),
        ("ullAvailVirtual", ctypes.c_ulonglong),
        ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
    ]


def free_ram_gb() -> float:
    """Свободная физическая память в ГБ (GlobalMemoryStatusEx)."""
    try:
        status = _MemoryStatus()
        status.dwLength = ctypes.sizeof(_MemoryStatus)
        if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            return status.ullAvailPhys / (1024**3)
    except Exception:  # pragma: no cover - не критично, просто отключаем проверку
        return float("inf")
    return float("inf")


@dataclass
class ModelEntry:
    model_id: str
    pipeline: Any
    lock: threading.Lock = field(default_factory=threading.Lock)
    last_used: float = field(default_factory=time.time)
    size_gb: float = 0.0


class ModelManager:
    def __init__(self, registry: Registry):
        self.registry = registry
        self._entries: dict[str, ModelEntry] = {}
        self._embedders: dict[str, Any] = {}
        self._rerankers: dict[str, Any] = {}
        self._vlms: dict[str, Any] = {}
        self._images: dict[str, Any] = {}
        self._global_lock = threading.Lock()
        self._cache_lock = threading.Lock()

    # ------------------------------------------------------------------ служебное
    def loaded_ids(self) -> list[str]:
        with self._global_lock:
            return list(self._entries)

    def _config(self, device: str) -> dict[str, Any]:
        # Кэш компиляции обязателен: первый запуск 10–45 с, дальше мгновенно.
        config: dict[str, Any] = {"CACHE_DIR": self.registry.cache_dir, "PERFORMANCE_HINT": "LATENCY"}
        if device.upper().startswith("NPU"):
            # У NPU по умолчанию лимит 1024 токена на промпт — для дебатов и длинных
            # чатов этого мало (ошибка «may only process prompts up to 1024 tokens»).
            config["MAX_PROMPT_LEN"] = 4096
            config["MIN_RESPONSE_LEN"] = 256
        return config

    def _load(self, model_id: str) -> ModelEntry:
        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        path = cfg["path"]
        device = cfg.get("device", "GPU")
        size_gb = float(cfg.get("size_gb", 0))
        if device == "NPU" and size_gb > NPU_MAX_GB:
            raise ModelError(f"Модель {model_id} слишком велика для NPU ({size_gb} ГБ)")

        free = free_ram_gb()
        if free < size_gb + 2:
            # Совет/дебаты: после участников их модели ещё висят в кэше.
            # Перед загрузкой арбитра выгружаем всё лишнее, иначе 14B не влезает.
            self._evict_all_except(model_id)
            free = free_ram_gb()
        if free < size_gb + 2:
            raise ModelError(
                f"Недостаточно свободной памяти: нужно {size_gb + 2:.1f} ГБ, доступно {free:.1f} ГБ. "
                "Закройте лишние приложения или выберите модель поменьше."
            )

        try:
            print(f"[manager] creating LLMPipeline {path} on {device}", flush=True)
            pipeline = ov_genai.LLMPipeline(path, device, self._config(device))
            print("[manager] pipeline created", flush=True)
        except Exception as exc:  # noqa: BLE001 - текст ошибки важен пользователю
            raise ModelError(f"Не удалось загрузить {cfg['label']} на {device}: {exc}") from exc

        entry = ModelEntry(model_id=model_id, pipeline=pipeline, size_gb=size_gb)
        with self._global_lock:
            self._entries[model_id] = entry
        return entry

    def _evict_all_except(self, model_id: str) -> None:
        """Освобождает RAM перед загрузкой: выгружает все другие модели и кэши."""
        with self._global_lock:
            victims = [e for e in self._entries.values() if e.model_id != model_id]
            for entry in victims:
                self._entries.pop(entry.model_id, None)
        for entry in victims:
            entry.pipeline = None
        # _load вызывается под _cache_lock; повторно его брать нельзя (deadlock).
        self._vlms.clear()
        self._images.clear()
        import gc

        gc.collect()
        time.sleep(0.5)

    def _evict_for_locked(self, model_id: str, size_gb: float) -> None:
        """Выгружает LRU. Вызывать только под уже взятым self._global_lock.

        Раньше здесь брался тот же нереентерабельный Lock повторно — это давало
        вечный deadlock при загрузке любой модели тяжелее 8 ГБ.
        """
        if size_gb <= HEAVY_THRESHOLD_GB:
            return
        heavy = [
            e for e in self._entries.values() if e.size_gb > HEAVY_THRESHOLD_GB and e.model_id != model_id
        ]
        heavy.sort(key=lambda e: e.last_used)
        for entry in heavy:
            self._entries.pop(entry.model_id, None)

    def ensure_loaded(self, model_id: str) -> ModelEntry:
        print(f"[manager] ensure_loaded {model_id}", flush=True)
        with self._global_lock:
            entry = self._entries.get(model_id)
            if entry is not None:
                entry.last_used = time.time()
                return entry
        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        with self._cache_lock:  # компиляция/загрузка — по одной
            with self._global_lock:
                if model_id in self._entries:
                    return self._entries[model_id]
                self._evict_for_locked(model_id, float(cfg.get("size_gb", 0)))
            return self._load(model_id)

    def load(self, model_id: str) -> dict[str, Any]:
        started = time.time()
        self.ensure_loaded(model_id)
        return {"loaded": True, "load_seconds": round(time.time() - started, 2)}

    def unload(self, model_id: str) -> dict[str, Any]:
        with self._global_lock:
            entry = self._entries.pop(model_id, None)
        if entry is not None:
            with entry.lock:
                entry.pipeline = None
        return {"loaded": False}

    def unload_all(self) -> None:
        with self._global_lock:
            entries = list(self._entries.values())
            self._entries.clear()
        for entry in entries:
            with entry.lock:
                entry.pipeline = None

    # ------------------------------------------------------------------ эмбеддинги
    def embed(self, model_id: str, texts: list[str]) -> list[list[float]]:
        """Векторы для RAG. Модель должна быть добавлена как локальная OpenVINO IR."""
        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        if not texts:
            return []
        device = str(cfg.get("device", "CPU"))
        with self._cache_lock:
            pipeline = self._embedders.get(model_id)
            if pipeline is None:
                try:
                    pipeline = ov_genai.TextEmbeddingPipeline(cfg["path"], device)
                    self._embedders[model_id] = pipeline
                except Exception as exc:  # noqa: BLE001
                    raise ModelError(
                        f"Не удалось загрузить эмбеддинг-модель {cfg.get('label', model_id)}: {exc}"
                    ) from exc
        try:
            vectors = pipeline.embed_documents(texts)
        except Exception as exc:  # noqa: BLE001
            raise ModelError(f"Ошибка эмбеддингов: {exc}") from exc
        return [[float(value) for value in vector] for vector in vectors]

    # ------------------------------------------------------------------ реранкер
    def rerank(
        self, model_id: str, query: str, documents: list[str]
    ) -> list[tuple[int, float]]:
        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        if not documents:
            return []
        device = str(cfg.get("device", "CPU"))
        with self._cache_lock:
            pipeline = self._rerankers.get(model_id)
            if pipeline is None:
                try:
                    pipeline = ov_genai.TextRerankPipeline(cfg["path"], device)
                    self._rerankers[model_id] = pipeline
                except Exception as exc:  # noqa: BLE001
                    raise ModelError(
                        f"Не удалось загрузить реранкер {cfg.get('label', model_id)}: {exc}"
                    ) from exc
        try:
            results = pipeline.rerank(query, documents)
        except Exception as exc:  # noqa: BLE001
            raise ModelError(f"Ошибка реранкера: {exc}") from exc
        return [(int(index), float(score)) for index, score in results]

    # ------------------------------------------------------------------ зрение
    def vlm_generate(
        self,
        model_id: str,
        prompt: str,
        images: list[Any],
        max_tokens: int,
        temperature: float,
        cancel_check: Callable[[], bool] | None = None,
    ) -> Iterator[tuple[str, Any]]:
        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        if not cfg.get("vlm"):
            raise ModelError("Модель не поддерживает изображения")
        device = str(cfg.get("device", "CPU"))
        with self._cache_lock:
            pipeline = self._vlms.get(model_id)
            if pipeline is None:
                try:
                    pipeline = ov_genai.VLMPipeline(cfg["path"], device)
                    self._vlms[model_id] = pipeline
                except Exception as exc:  # noqa: BLE001
                    raise ModelError(f"Не удалось загрузить VLM: {exc}") from exc
        config = ov_genai.GenerationConfig()
        config.max_new_tokens = max(1, int(max_tokens))
        if temperature and temperature > 0.01:
            config.do_sample = True
            config.temperature = float(temperature)
            config.top_p = 0.9
        else:
            config.do_sample = False

        import queue

        q: "queue.Queue[Any]" = queue.Queue()
        stop = threading.Event()
        started = time.time()

        def streamer(chunk: str) -> bool:
            if stop.is_set():
                return True
            if cancel_check is not None and cancel_check():
                stop.set()
                return True
            q.put(chunk)
            return False

        def worker() -> None:
            try:
                if images:
                    pipeline.generate(prompt, images, config, streamer)
                else:
                    pipeline.generate(prompt, generation_config=config, streamer=streamer)
                q.put(_END)
            except Exception as exc:  # noqa: BLE001
                q.put(exc)

        thread = threading.Thread(target=worker, daemon=True)
        thread.start()
        tokens = 0
        while True:
            item = q.get()
            if item is _END:
                break
            if isinstance(item, Exception):
                raise ModelError(f"Ошибка VLM: {item}") from item
            if stop.is_set():
                break
            tokens += 1
            yield ("delta", item)
        thread.join(timeout=1.0)
        yield ("done", tokens, round(time.time() - started, 2))

    # ------------------------------------------------------------------ картинки
    def generate_image(
        self,
        model_id: str,
        prompt: str,
        negative: str,
        width: int,
        height: int,
        steps: int,
        seed: int,
        guidance_scale: float = 7.5,
    ) -> bytes:
        import io

        import numpy as np
        from PIL import Image

        cfg = self.registry.get(model_id)
        if cfg is None:
            raise ModelError(f"Неизвестная модель: {model_id}")
        device = str(cfg.get("device", "CPU"))
        with self._cache_lock:
            pipeline = self._images.get(model_id)
            if pipeline is None:
                try:
                    pipeline = ov_genai.Text2ImagePipeline(cfg["path"], device)
                    self._images[model_id] = pipeline
                except Exception as exc:  # noqa: BLE001
                    raise ModelError(f"Не удалось загрузить image-модель: {exc}") from exc
        try:
            tensor = pipeline.generate(
                prompt,
                negative_prompt=negative,
                width=max(128, int(width)),
                height=max(128, int(height)),
                num_inference_steps=max(1, int(steps)),
                guidance_scale=max(1.0, float(guidance_scale)),
                rng_seed=int(seed),
            )
            array = np.asarray(tensor.data if hasattr(tensor, "data") else tensor)
            if array.dtype == object:
                array = np.stack([np.asarray(item) for item in array.flat])
            if array.ndim == 4:
                array = array[0]
            if array.dtype != np.uint8:
                array = np.clip(array, 0, 255).astype(np.uint8)
            image = Image.fromarray(array)
            out = io.BytesIO()
            image.save(out, format="PNG")
            return out.getvalue()
        except Exception as exc:  # noqa: BLE001
            raise ModelError(f"Ошибка генерации изображения: {exc}") from exc

    # ------------------------------------------------------------------ генерация
    def generate_stream(
        self,
        model_id: str,
        messages: list[dict[str, str]],
        max_tokens: int,
        temperature: float,
        cancel_check: Callable[[], bool] | None = None,
    ) -> Iterator[tuple[str, Any]]:
        """Отдаёт поток событий ('delta', text) ... ('done', tokens, seconds)."""
        entry = self.ensure_loaded(model_id)
        prompt = render_chatml(messages)
        config = ov_genai.GenerationConfig()
        config.max_new_tokens = max(1, int(max_tokens))
        if temperature and temperature > 0.01:
            config.do_sample = True
            config.temperature = float(temperature)
            config.top_p = 0.9
        else:
            config.do_sample = False
        try:
            config.stop_strings = ov_genai.StringArray(["<|im_end|>"])
        except Exception:  # noqa: BLE001 - в разных сборках тип называется иначе
            pass

        started = time.time()
        tokens = 0
        with entry.lock:
            for chunk in self._stream(entry, prompt, config, cancel_check):
                tokens += 1
                yield ("delta", chunk)
        yield ("done", tokens, round(time.time() - started, 2))

    def _stream(self, entry: ModelEntry, prompt: str, config: Any, cancel_check: Callable[[], bool] | None):
        """OpenVINO отдаёт колбэк из своего потока — прокидываем чанки через очередь."""
        import queue

        q: "queue.Queue[Any]" = queue.Queue()
        stop = threading.Event()

        def streamer(chunk: str) -> bool:
            if stop.is_set():
                return True
            if cancel_check is not None and cancel_check():
                stop.set()
                return True
            q.put(chunk)
            return False

        def worker() -> None:
            try:
                entry.pipeline.generate(prompt, config, streamer)
                q.put(_END)
            except Exception as exc:  # noqa: BLE001
                q.put(exc)

        thread = threading.Thread(target=worker, daemon=True)
        thread.start()
        while True:
            item = q.get()
            if item is _END:
                break
            if isinstance(item, Exception):
                raise ModelError(f"Ошибка генерации: {item}") from item
            if stop.is_set():
                break
            yield item
        thread.join(timeout=1.0)


class _End:
    pass


class TokenizerCache:
    """Кэш токенизаторов: считаем токены в промпте без загрузки самой LLM.

    Токенизатор лёгкий (файлы из папки модели), но не потокобезопасный,
    поэтому все вызовы encode идут под RLock.
    """

    def __init__(self, registry: Registry):
        self.registry = registry
        self._cache: dict[str, Any] = {}
        self._lock = threading.RLock()

    def _get(self, model_id: str) -> Any:
        with self._lock:
            tokenizer = self._cache.get(model_id)
            if tokenizer is not None:
                return tokenizer
            cfg = self.registry.get(model_id)
            if cfg is None:
                raise ModelError(f"Неизвестная модель: {model_id}")
            try:
                tokenizer = ov_genai.Tokenizer(cfg["path"])
            except Exception as exc:  # noqa: BLE001
                raise ModelError(f"Не удалось загрузить токенизатор {model_id}: {exc}") from exc
            self._cache[model_id] = tokenizer
            return tokenizer

    def count(self, model_id: str, texts: list[str]) -> list[int]:
        tokenizer = self._get(model_id)
        counts: list[int] = []
        with self._lock:
            for text in texts:
                encoded = tokenizer.encode(text)
                shape = list(encoded.input_ids.get_shape())
                counts.append(int(math.prod(shape)) if shape else 0)
        return counts


_END = _End()
