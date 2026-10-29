// Конструктор навыков: сборка текста навыка, пресеты и разбор SKILL.md.
import type { SkillInput } from "./types";

export interface SkillPreset {
  key: string;
  label: string;
  patch: Partial<SkillInput>;
}

export const SKILL_PRESETS: SkillPreset[] = [
  {
    key: "empty",
    label: "Пустой",
    patch: { name: "", description: "", role: "", guidelines: "", output_format: "", examples: "", constraints: "", triggers: "" },
  },
  {
    key: "brief",
    label: "Краткие ответы",
    patch: {
      name: "Кратко",
      description: "Отвечай максимально сжато, без вступлений",
      role: "Ты — предельно лаконичный ассистент.",
      guidelines: "Сначала суть в 1–2 предложениях. Если нужны детали — не более трёх коротких пунктов.",
      output_format: "1–2 предложения, при необходимости — до трёх пунктов.",
      examples: "",
      constraints: "Не пересказывай вопрос, не извиняйся, не добавляй вводных фраз.",
      triggers: "",
    },
  },
  {
    key: "review",
    label: "Код-ревью",
    patch: {
      name: "Код-ревью",
      description: "Разбор кода: риски, ошибки, улучшения по приоритету",
      role: "Ты — строгий, но доброжелательный ревьюер кода.",
      guidelines:
        "Сначала реальные проблемы (ошибки, гонки, утечки, безопасность), затем улучшения.\nДля каждой проблемы: что не так, почему важно, как исправить (с кодом).",
      output_format: "Список проблем с приоритетами critical / major / minor, затем итог и исправленный фрагмент.",
      examples: "",
      constraints: "Не придирайся к стилю без функциональной причины. Не выдумывай код, которого не видел.",
      triggers: "код, код-ревью, refactor, рефакторинг, python, rust, typescript",
    },
  },
  {
    key: "tutor",
    label: "Тьютор",
    patch: {
      name: "Тьютор",
      description: "Объясняет по шагам и проверяет понимание",
      role: "Ты — терпеливый преподаватель.",
      guidelines:
        "Разбей тему на шаги, объясняй простыми словами. После каждого шага — один проверочный вопрос. В конце — небольшое упражнение.",
      output_format: "Короткие абзацы, примеры, в конце — упражнение.",
      examples: "",
      constraints: "Не давай полное решение, пока ученик не попробовал сам.",
      triggers: "",
    },
  },
  {
    key: "translator",
    label: "Переводчик",
    patch: {
      name: "Переводчик",
      description: "Переводит текст, сохраняя смысл и термины",
      role: "Ты — профессиональный переводчик.",
      guidelines:
        "Определи язык исходника. Переведи на русский (или на язык, который просит пользователь). Сохраняй технические термины и форматирование.",
      output_format: "Только перевод, без пояснений. Если термин неоднозначен — короткая сноска в конце.",
      examples: "",
      constraints: "Не добавляй свои комментарии и не сокращай смысл.",
      triggers: "переведи, перевод, translate",
    },
  },
  {
    key: "summary",
    label: "Суммаризатор",
    patch: {
      name: "Суммаризатор",
      description: "Сжимает длинный текст до главного",
      role: "Ты — редактор, который делает выжимку.",
      guidelines: "Выдели цель текста, ключевые факты и решения. Убери повторы и воду.",
      output_format: "3–7 пунктов, затем одна итоговая фраза.",
      examples: "",
      constraints: "Не добавляй фактов, которых нет в исходнике.",
      triggers: "",
    },
  },
];

/** Собирает текст навыка так же, как это делает Rust (`skills::compose`). */
export function composeSkill(input: Partial<SkillInput>): string {
  const parts: string[] = [];
  const add = (value: string | null | undefined) => {
    const trimmed = (value ?? "").trim();
    if (trimmed) parts.push(trimmed);
  };
  add(input.description);
  if ((input.role ?? "").trim()) parts.push(`Роль: ${input.role!.trim()}`);
  add(input.guidelines);
  if ((input.output_format ?? "").trim()) parts.push(`Формат ответа: ${input.output_format!.trim()}`);
  if ((input.examples ?? "").trim()) parts.push(`Примеры:\n${input.examples!.trim()}`);
  if ((input.constraints ?? "").trim()) parts.push(`Ограничения: ${input.constraints!.trim()}`);
  if (parts.length === 0) return "";
  return `### ${(input.name ?? "").trim() || "Без названия"}\n${parts.join("\n\n")}`;
}

export interface ParsedSkill {
  name: string;
  description: string;
  guidelines: string;
}

/** Разбирает SKILL.md: YAML-frontmatter (name/description) + markdown-тело. */
export function parseSkillMd(text: string): ParsedSkill {
  const normalized = text.replace(/\r\n/g, "\n").trim();
  let name = "";
  let description = "";
  let body = normalized;
  const match = normalized.match(/^---\n([\s\S]*?)\n---\n?/);
  if (match) {
    const frontmatter = match[1];
    body = normalized.slice(match[0].length).trim();
    for (const line of frontmatter.split("\n")) {
      const pair = line.match(/^([A-Za-z_]+)\s*:\s*(.*)$/);
      if (!pair) continue;
      const key = pair[1].toLowerCase();
      const value = pair[2].trim().replace(/^["']|["']$/g, "");
      if (key === "name") name = value;
      if (key === "description") description = value;
    }
  }
  if (!name) {
    // Первый заголовок вида "# Название"
    const heading = body.match(/^#\s+(.+)$/m);
    if (heading) name = heading[1].trim();
  }
  return { name, description, guidelines: body };
}

/** Заполняет недостающие поля навыка из разобранного SKILL.md. */
export function skillFromMarkdown(text: string): Partial<SkillInput> {
  const parsed = parseSkillMd(text);
  return {
    name: parsed.name,
    description: parsed.description,
    guidelines: parsed.guidelines,
  };
}
