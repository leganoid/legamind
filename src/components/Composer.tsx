import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import type { PickedAttachment } from "../types";

interface Props {
  busy: boolean;
  disabled: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  attachments: PickedAttachment[];
  onAttach: () => void;
  onRemoveAttachment: (index: number) => void;
}

interface CommandHint {
  cmd: string;
  description: string;
  needsArg?: boolean;
}

const COMMANDS: CommandHint[] = [
  { cmd: "/compact", description: "сжать историю в сводку" },
  { cmd: "/clear", description: "очистить историю сессии" },
  { cmd: "/remember", description: "сохранить факт в память", needsArg: true },
  { cmd: "/skills", description: "список навыков" },
  { cmd: "/skill", description: "включить/выключить навык", needsArg: true },
  { cmd: "/export", description: "экспорт переписки в .md" },
  { cmd: "/pwd", description: "рабочая папка проекта" },
  { cmd: "/workdir", description: "задать рабочую папку", needsArg: true },
  { cmd: "/web", description: "поиск в интернете", needsArg: true },
  { cmd: "/index", description: "проиндексировать рабочую папку" },
  { cmd: "/help", description: "список команд" },
];

const PRESETS: { label: string; prefix: string }[] = [
  { label: "Код", prefix: "Разбери код и предложи улучшения:\n" },
  { label: "Анализ", prefix: "Проанализируй и выдели главное:\n" },
  { label: "Перевод", prefix: "Переведи на русский, сохранив смысл:\n" },
  { label: "Резюме", prefix: "Сделай краткое резюме:\n" },
  { label: "План", prefix: "Составь пошаговый план:\n" },
];

/** Ввод: Enter — отправить, Shift+Enter — перенос, Ctrl+Enter — тоже отправить. */
export default function Composer(props: Props) {
  const [text, setText] = createSignal("");
  const [hintIndex, setHintIndex] = createSignal(0);
  let inputRef: HTMLTextAreaElement | undefined;

  const hints = createMemo(() => {
    const value = text();
    if (!value.startsWith("/") || value.includes("\n") || value.includes(" ")) return [];
    return COMMANDS.filter((hint) => hint.cmd.startsWith(value));
  });

  const focusComposer = () => {
    inputRef?.focus();
  };

  onMount(() => {
    window.addEventListener("legamind:focus-composer", focusComposer);
  });
  onCleanup(() => {
    window.removeEventListener("legamind:focus-composer", focusComposer);
  });

  const submit = () => {
    const value = text().trim();
    if (!value || props.busy || props.disabled) return;
    props.onSend(value);
    setText("");
    if (inputRef) inputRef.style.height = "auto";
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      setText("");
      return;
    }
    const list = hints();
    if (list.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setHintIndex((index) => (index + 1) % list.length);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setHintIndex((index) => (index - 1 + list.length) % list.length);
        return;
      }
      if (event.key === "Tab") {
        event.preventDefault();
        useHint(list[hintIndex() % list.length]);
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submit();
    }
  };

  const autoGrow = (event: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
    const el = event.currentTarget;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 220) + "px";
  };

  const useHint = (hint: CommandHint) => {
    setText(hint.needsArg ? `${hint.cmd} ` : `${hint.cmd} `);
    setHintIndex(0);
    inputRef?.focus();
  };

  const applyPreset = (prefix: string) => {
    const current = text().trim();
    setText(current ? `${prefix}${current}` : prefix);
    inputRef?.focus();
  };

  return (
    <div class="composer">
      <Show when={hints().length > 0}>
        <div class="command-hints">
          <For each={hints()}>
            {(hint, index) => (
              <button
                classList={{
                  "command-hint": true,
                  active: hintIndex() % hints().length === index(),
                }}
                onClick={() => useHint(hint)}
              >
                <span class="command-name">{hint.cmd}</span>
                <span class="muted small">{hint.description}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
      <Show when={props.attachments.length > 0}>
        <div class="attachment-chips">
          <span class="muted small">Вложения:</span>
          <For each={props.attachments}>
            {(file, index) => (
              <span class="attachment-chip">
                {file.kind === "pdf"
                  ? "📕"
                  : file.kind === "image"
                    ? "🖼"
                    : file.kind === "binary"
                      ? "🧩"
                      : "📄"}{" "}
                {file.name}
                <span class="muted small"> · {(file.size / 1024).toFixed(1)} КБ</span>
                <button
                  class="icon-btn"
                  title="Убрать вложение"
                  onClick={() => props.onRemoveAttachment(index())}
                >
                  ✕
                </button>
              </span>
            )}
          </For>
        </div>
      </Show>
      <textarea
        ref={inputRef}
        value={text()}
        spellcheck={true}
        lang="ru-RU"
        placeholder="Сообщение… (Enter — отправить, Shift+Enter — перенос, /help — команды)"
        rows={2}
        disabled={props.disabled}
        onInput={(e) => {
          setText(e.currentTarget.value);
          autoGrow(e);
        }}
        onKeyDown={onKeyDown}
      />
      <div class="composer-actions">
        <details class="menu">
          <summary class="btn ghost small">✨ Пресеты</summary>
          <div class="menu-items">
            <For each={PRESETS}>
              {(preset) => (
                <button onClick={() => applyPreset(preset.prefix)}>{preset.label}</button>
              )}
            </For>
          </div>
        </details>
        <button
          class="btn ghost small"
          title="Приложить файл: текст, PDF, изображение или бинарник"
          onClick={() => props.onAttach()}
          disabled={props.busy || props.disabled}
        >
          📎 Файл
        </button>
        {props.busy ? (
          <button class="btn danger" onClick={() => props.onStop()}>
            Стоп
          </button>
        ) : (
          <button class="btn primary" disabled={props.disabled || !text().trim()} onClick={submit}>
            Отправить
          </button>
        )}
      </div>
    </div>
  );
}
