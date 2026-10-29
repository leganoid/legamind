"""Шаблоны ChatML и системные промпты.

Qwen3 по умолчанию «думает»; /no_think в конце вопроса отключает рассуждения.
"""

from __future__ import annotations

from typing import Any, Iterable

IM_END = "<|im_end|>"
DEFAULT_SYSTEM = "Ты — полезный ассистент. Отвечай по-русски, кратко и по делу."


def render_chatml(messages: Iterable[dict[str, str]]) -> str:
    """Собирает ChatML-промпт ровно в формате задания."""
    parts: list[str] = []
    for message in messages:
        role = message.get("role", "user")
        content = message.get("content", "")
        parts.append(f"<|im_start|>{role}\n{content}{IM_END}\n")
    parts.append("<|im_start|>assistant\n")
    return "".join(parts)


def with_default_system(messages: list[dict[str, str]], extra_system: str | None = None) -> list[dict[str, str]]:
    """Гарантирует наличие system-сообщения и язык ответа."""
    system = DEFAULT_SYSTEM if extra_system is None else extra_system
    if extra_system is not None and "по-русски" not in extra_system.lower():
        system = extra_system.rstrip() + " Отвечай по-русски."
    result = [{"role": "system", "content": system}]
    result.extend(m for m in messages if m.get("role") != "system")
    return result


def apply_think(prompt: str, thinks: bool, think: bool) -> str:
    """Qwen3: добавляет /no_think, когда рассуждения не нужны и модель умеет думать."""
    if not thinks or think:
        return prompt
    return prompt + " /no_think"


def memory_block(facts: list[str]) -> str | None:
    """Блок «Известное о проекте» из раздела 7.2 с бюджетом ~3000 символов."""
    if not facts:
        return None
    budget = 3000
    header = "Известное о проекте (используй, если релевантно):\n"
    lines: list[str] = []
    used = len(header)
    for fact in facts:
        line = f"- {fact.strip()}\n"
        if used + len(line) > budget:
            break
        lines.append(line)
        used += len(line)
    if not lines:
        return None
    return header + "".join(lines).strip()


def debate_proposal_prompt(model_label: str, task: str, max_tokens: int) -> str:
    return (
        f"Ты — {model_label}, участник обсуждения. Задача пользователя:\n"
        "---\n"
        f"{task}\n"
        "---\n"
        "Предложи решение. Будь конкретен: шаги, структура, ключевые решения.\n"
        "Не обращайся к другим участникам. Не пересказывай задачу. Отвечай по-русски.\n"
        f"Максимум {max_tokens} токенов."
    )


def debate_critique_prompt(own_previous: str, other_proposals: str) -> str:
    return (
        "Твоё предложение:\n"
        "---\n"
        f"{own_previous}\n"
        "---\n"
        "Предложения других участников:\n"
        f"{other_proposals}\n\n"
        "Критично оцени все предложения, включая своё. Найди слабые места и риски.\n"
        "Затем предложи улучшенный план. Не повторяй чужие формулировки. По-русски."
    )


def judge_prompt(task: str, final_proposals: str) -> str:
    return (
        "Задача:\n"
        "---\n"
        f"{task}\n"
        "---\n"
        "Предложения после обсуждения:\n"
        f"{final_proposals}\n\n"
        "Ты — арбитр. Выбери лучшую стратегию и объясни выбор в 3–5 предложениях.\n"
        "Затем выдай итоговое решение: конкретный план/текст/код, который пользователь может применить.\n"
        "Не упоминай «участник A/B» — говори по существу. По-русски."
    )


def vote_prompt(task: str, proposals: dict[str, str]) -> str:
    listing = "\n\n".join(f"{name}:\n{text}" for name, text in proposals.items())
    return (
        "Задача:\n"
        "---\n"
        f"{task}\n"
        "---\n"
        f"Предложения участников:\n{listing}\n\n"
        "Оцени каждое предложение (кроме своего) по шкале 1–10.\n"
        'Ответь строго JSON без пояснений: {"scores":{"Участник A":7,"Участник B":5}}'
    )


def anon_name(index: int) -> str:
    """«Участник A», «Участник B», ... — реальные имена скрыты от участников."""
    return f"Участник {chr(ord('A') + index)}"


def build_messages(system: str, prompt: str) -> list[dict[str, Any]]:
    return [{"role": "system", "content": system}, {"role": "user", "content": prompt}]
