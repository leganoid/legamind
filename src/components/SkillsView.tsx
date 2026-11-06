import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { Skill, SkillInput } from "../types";
import { composeSkill, skillFromMarkdown, SKILL_PRESETS } from "../skills";
import { SKILL_LIBRARY } from "../skill-library";

const EMPTY: SkillInput = {
  id: null,
  name: "",
  description: "",
  role: "",
  guidelines: "",
  output_format: "",
  examples: "",
  constraints: "",
  triggers: "",
  enabled: false,
};

export default function SkillsView() {
  const [skills, setSkills] = createSignal<Skill[]>([]);
  const [editing, setEditing] = createSignal<SkillInput | null>(null);
  const [importOpen, setImportOpen] = createSignal(false);
  const [libraryOpen, setLibraryOpen] = createSignal(false);
  const [libraryQuery, setLibraryQuery] = createSignal("");
  const [importText, setImportText] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);

  const load = async () => setSkills(await api.skillsList());

  const startNew = () => {
    setError(null);
    setNotice(null);
    setEditing({ ...EMPTY });
  };

  const startEdit = (skill: Skill) => {
    setError(null);
    setNotice(null);
    setEditing({
      id: skill.id,
      name: skill.name,
      description: skill.description ?? "",
      role: skill.role ?? "",
      guidelines: skill.guidelines ?? "",
      output_format: skill.output_format ?? "",
      examples: skill.examples ?? "",
      constraints: skill.constraints ?? "",
      triggers: skill.triggers ?? "",
      enabled: skill.enabled,
    });
  };

  const patch = (field: keyof SkillInput, value: string) => {
    setEditing((prev) => (prev ? { ...prev, [field]: value } : prev));
  };

  const applyPreset = (key: string) => {
    const preset = SKILL_PRESETS.find((item) => item.key === key);
    if (!preset) return;
    setEditing((prev) => ({ ...(prev ?? EMPTY), ...preset.patch, id: prev?.id ?? null }));
  };

  const applyLibrary = (key: string) => {
    const item = SKILL_LIBRARY.find((entry) => entry.key === key);
    if (!item) return;
    setError(null);
    setNotice(`«${item.label}» загружен в конструктор — проверьте и сохраните`);
    setEditing({ ...EMPTY, ...item.patch, enabled: false });
    setLibraryOpen(false);
  };

  const filteredLibrary = () => {
    const query = libraryQuery().trim().toLowerCase();
    if (!query) return SKILL_LIBRARY;
    return SKILL_LIBRARY.filter((item) =>
      `${item.label} ${item.summary} ${item.category} ${item.patch.triggers ?? ""}`
        .toLowerCase()
        .includes(query),
    );
  };

  const preview = createMemo(() => {
    const current = editing();
    return current ? composeSkill(current) : "";
  });

  const save = async () => {
    const current = editing();
    if (!current) return;
    if (!current.name?.trim()) {
      setError("Укажите название скилла");
      return;
    }
    if ((current.guidelines ?? "").trim().length < 20) {
      setError("Инструкции слишком короткие: опишите шаги хотя бы одним предложением");
      return;
    }
    if (preview().length > 4000) {
      setError("Скилл длиннее 4000 символов — он не поместится в бюджет промпта");
      return;
    }
    try {
      await api.skillSave(current);
      setEditing(null);
      setNotice("Навык сохранён");
      await load();
    } catch (err) {
      setError(String(err));
    }
  };

  const toggle = async (skill: Skill) => {
    await api.skillSetEnabled(skill.id, !skill.enabled);
    await load();
  };

  const remove = async (skill: Skill, event: MouseEvent) => {
    event.stopPropagation();
    if (!window.confirm(`Удалить навык «${skill.name}»?`)) return;
    await api.skillDelete(skill.id);
    await load();
  };

  const parseImport = () => {
    const parsed = skillFromMarkdown(importText());
    if (!parsed.name && !parsed.guidelines) {
      setError("Не удалось разобрать SKILL.md: нужен frontmatter или заголовок # Название");
      return;
    }
    setEditing({ ...EMPTY, ...parsed, enabled: false });
    setImportOpen(false);
    setImportText("");
    setNotice("Разобрано: проверьте поля и сохраните");
  };

  onMount(load);

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Скиллы</h1>
          <p class="muted small">
            Навык — это набор инструкций (роль, шаги, формат, примеры, ограничения), который
            добавляется в system-промпт. Включённые навыки работают во всех сессиях; если заданы
            триггеры — только при совпадении с сообщением.
          </p>
        </div>
        <div class="row">
          <button class="btn" onClick={() => setLibraryOpen((v) => !v)}>
            📚 Библиотека
          </button>
          <button class="btn" onClick={() => setImportOpen((v) => !v)}>
            Импорт SKILL.md
          </button>
          <button class="btn primary" onClick={startNew}>
            🛠 Конструктор
          </button>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

      <Show when={libraryOpen()}>
        <section class="card">
          <div class="row space-between">
            <h2>Библиотека готовых скиллов</h2>
            <button class="btn ghost small" onClick={() => setLibraryOpen(false)}>
              Закрыть
            </button>
          </div>
          <p class="muted small">
            Проверенные шаблоны: загрузите в конструктор, проверьте под свой проект и сохраните.
            Скилл начнёт работать после включения.
          </p>
          <input
            placeholder="Поиск: код, SQL, безопасность, документы…"
            value={libraryQuery()}
            onInput={(event) => setLibraryQuery(event.currentTarget.value)}
          />
          <div class="card-grid">
            <For each={filteredLibrary()}>
              {(item) => (
                <section class="card model-card">
                  <div class="row space-between">
                    <strong>{item.label}</strong>
                    <span class="badge">{item.category}</span>
                  </div>
                  <p class="muted small">{item.summary}</p>
                  <button class="btn small" onClick={() => applyLibrary(item.key)}>
                    В конструктор
                  </button>
                </section>
              )}
            </For>
          </div>
        </section>
      </Show>

      <Show when={importOpen()}>
        <section class="card form">
          <h2>Импорт SKILL.md</h2>
          <p class="muted small">
            Вставьте содержимое файла навыка. Frontmatter `name`/`description` и тело markdown
            разложатся по полям конструктора.
          </p>
          <textarea
            rows={10}
            class="mono-area"
            placeholder={"---\nname: pdf-processing\ndescription: ...\n---\n\n# PDF Processing\n..."}
            value={importText()}
            onInput={(e) => setImportText(e.currentTarget.value)}
          />
          <div class="form-actions">
            <button class="btn primary" onClick={parseImport} disabled={!importText().trim()}>
              Разобрать
            </button>
            <button class="btn ghost" onClick={() => setImportOpen(false)}>
              Отмена
            </button>
          </div>
        </section>
      </Show>

      <Show when={editing()}>
        <section class="card form skill-editor">
          <div class="row space-between">
            <h2>{editing()!.id ? "Изменить навык" : "Новый навык"}</h2>
            <label class="inline">
              Пресет
              <select onChange={(e) => applyPreset(e.currentTarget.value)} value="">
                <option value="">выбрать…</option>
                <For each={SKILL_PRESETS}>{(preset) => <option value={preset.key}>{preset.label}</option>}</For>
              </select>
            </label>
          </div>

          <div class="form-grid">
            <label>
              Название
              <input
                value={editing()!.name}
                onInput={(e) => patch("name", e.currentTarget.value)}
                placeholder="Например: Код-ревью"
              />
            </label>
            <label>
              Когда применять (описание)
              <input
                value={editing()!.description ?? ""}
                onInput={(e) => patch("description", e.currentTarget.value)}
                placeholder="Краткое описание для списка и промпта"
              />
            </label>
            <label>
              Триггеры (через запятую)
              <input
                value={editing()!.triggers ?? ""}
                onInput={(e) => patch("triggers", e.currentTarget.value)}
                placeholder="код, рефакторинг, python (пусто = всегда)"
              />
            </label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                checked={!!editing()!.enabled}
                onChange={(e) => setEditing((prev) => (prev ? { ...prev, enabled: e.currentTarget.checked } : prev))}
              />
              Включён сразу
            </label>
          </div>

          <label class="stack">
            Роль (кто ты)
            <textarea
              rows={2}
              value={editing()!.role ?? ""}
              onInput={(e) => patch("role", e.currentTarget.value)}
              placeholder="Ты — строгий ревьюер кода…"
            />
          </label>
          <label class="stack">
            Инструкции / шаги
            <textarea
              rows={5}
              value={editing()!.guidelines ?? ""}
              onInput={(e) => patch("guidelines", e.currentTarget.value)}
              placeholder="Что делать по шагам…"
            />
          </label>
          <div class="form-grid">
            <label>
              Формат ответа
              <textarea
                rows={3}
                value={editing()!.output_format ?? ""}
                onInput={(e) => patch("output_format", e.currentTarget.value)}
                placeholder="Списки, таблицы, код в блоках…"
              />
            </label>
            <label>
              Примеры
              <textarea
                rows={3}
                value={editing()!.examples ?? ""}
                onInput={(e) => patch("examples", e.currentTarget.value)}
                placeholder="Вопрос → ответ (по одному на строку)"
              />
            </label>
            <label>
              Ограничения
              <textarea
                rows={3}
                value={editing()!.constraints ?? ""}
                onInput={(e) => patch("constraints", e.currentTarget.value)}
                placeholder="Чего не делать…"
              />
            </label>
          </div>

          <div class="skill-preview">
            <div class="row space-between">
              <span class="muted small">Предпросмотр того, что уйдёт в system-промпт</span>
              <span classList={{ "muted": true, small: true, "text-danger": preview().length > 4000 }}>
                {preview().length} / 4000 символов
              </span>
            </div>
            <pre class="preview-box">{preview() || "Пока пусто — заполните поля"}</pre>
            <Show when={!editing()!.triggers?.trim()}>
              <p class="muted small">
                Триггеры не заданы: скилл будет активен во всех сообщениях. Для узкого навыка
                добавьте ключевые слова.
              </p>
            </Show>
          </div>

          <div class="form-actions">
            <button class="btn primary" onClick={save} disabled={!preview()}>
              Сохранить
            </button>
            <button class="btn ghost" onClick={() => setEditing(null)}>
              Отмена
            </button>
          </div>
        </section>
      </Show>

      <Show when={skills().length === 0 && !editing()}>
        <p class="empty-inline">Навыков пока нет. Создайте первый или импортируйте SKILL.md.</p>
      </Show>

      <div class="skill-list">
        <For each={skills()}>
          {(skill) => (
            <section class="card skill-card">
              <div class="skill-main">
                <h3>
                  {skill.name}
                  <Show when={skill.builtin}>
                    <span class="badge">встроенный</span>
                  </Show>
                  <span classList={{ badge: true, on: skill.enabled, off: !skill.enabled }}>
                    {skill.enabled ? "включён" : "выключен"}
                  </span>
                </h3>
                <Show when={skill.description}>
                  <p class="muted small">{skill.description}</p>
                </Show>
                <Show when={skill.triggers}>
                  <p class="muted small">Триггеры: {skill.triggers}</p>
                </Show>
                <details class="skill-details">
                  <summary class="muted small">Показать промпт навыка</summary>
                  <pre class="preview-box">{skill.preview}</pre>
                </details>
              </div>
              <div class="skill-actions">
                <button class="btn small" onClick={() => toggle(skill)}>
                  {skill.enabled ? "Выключить" : "Включить"}
                </button>
                <button class="btn small" onClick={() => startEdit(skill)}>
                  Изменить
                </button>
                <button class="btn small danger" onClick={(e) => remove(skill, e)}>
                  Удалить
                </button>
              </div>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
