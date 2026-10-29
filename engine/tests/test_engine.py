"""Тесты движка: реестр моделей, шаблоны промптов."""

from __future__ import annotations

import json
from pathlib import Path

from engine import prompts
from engine.manager import free_ram_gb
from engine.registry import MODELS, Registry


def test_registry_defaults():
    registry = Registry()
    assert set(registry.ids()) == {"qwen3-14b", "coder-30b", "coder-7b", "qwen-1.5b"}
    assert registry.get("qwen-1.5b")["device"] == "NPU"
    assert registry.get("qwen3-14b")["thinks"] is True


def test_registry_as_list_marks_loaded():
    registry = Registry()
    items = registry.as_list({"qwen-1.5b"})
    loaded = [item for item in items if item["loaded"]]
    assert len(loaded) == 1
    assert loaded[0]["id"] == "qwen-1.5b"


def test_registry_from_config(tmp_path: Path):
    config = tmp_path / "engine.json"
    config.write_text(
        json.dumps({"cache_dir": r"D:\cache", "models": {"tiny": {"path": "D:\\tiny", "label": "Tiny", "device": "CPU", "size_gb": 0.1, "max_tokens": 128}}}),
        encoding="utf-8",
    )
    registry = Registry.from_config_file(config)
    assert registry.ids() == ["tiny"]
    assert registry.cache_dir == r"D:\cache"


def test_registry_bad_config_falls_back(tmp_path: Path):
    broken = tmp_path / "broken.json"
    broken.write_text("{not json", encoding="utf-8")
    registry = Registry.from_config_file(broken)
    assert registry.models == MODELS


def test_chatml_format():
    text = prompts.render_chatml(
        [
            {"role": "system", "content": "s"},
            {"role": "user", "content": "u"},
        ]
    )
    assert text == (
        "<|im_start|>system\ns<|im_end|>\n"
        "<|im_start|>user\nu<|im_end|>\n"
        "<|im_start|>assistant\n"
    )


def test_default_system_forces_russian():
    messages = prompts.with_default_system([{"role": "user", "content": "привет"}])
    assert messages[0]["role"] == "system"
    assert "по-русски" in messages[0]["content"].lower()


def test_apply_think_for_qwen3():
    assert prompts.apply_think("вопрос", thinks=True, think=False).endswith("/no_think")
    assert prompts.apply_think("вопрос", thinks=True, think=True) == "вопрос"
    assert prompts.apply_think("вопрос", thinks=False, think=False) == "вопрос"


def test_memory_block_budget():
    facts = ["факт " + str(i) * 200 for i in range(30)]
    block = prompts.memory_block(facts)
    assert block is not None
    assert len(block) <= 3100
    assert block.startswith("Известное о проекте")


def test_memory_block_empty():
    assert prompts.memory_block([]) is None


def test_debate_prompts_contain_task_and_anonymity():
    proposal = prompts.debate_proposal_prompt("Qwen3-14B", "задача X", 512)
    assert "Qwen3-14B" in proposal and "задача X" in proposal and "512" in proposal
    critique = prompts.debate_critique_prompt("моё", "Участник A:\nчужое")
    assert "Участник A" in critique
    judge = prompts.judge_prompt("задача X", "Участник A:\nответ")
    assert "арбитр" in judge


def test_free_ram_is_positive():
    assert free_ram_gb() > 0
