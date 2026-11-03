import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import type { ModelInfo } from "../types";

export default function QuickAsk() {
  const [models, setModels] = createSignal<ModelInfo[]>([]);
  const [modelId, setModelId] = createSignal("");
  const [prompt, setPrompt] = createSignal("");
  const [answer, setAnswer] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let area: HTMLTextAreaElement | undefined;

  const localModels = () => models().filter((model) => model.kind === "local");
  const cloudModels = () => models().filter((model) => model.kind !== "local");

  const loadModels = async () => {
    try {
      const all = await api.modelsList();
      const chat = all.filter(
        (model) =>
          model.role === "chat" ||
          (model.kind === "local" &&
            model.role !== "embedding" &&
            model.role !== "rerank"),
      );
      setModels(chat);
      if (!chat.some((model) => model.id === modelId())) {
        setModelId((chat.find((model) => model.kind === "local") ?? chat[0])?.id ?? "");
      }
    } catch (err) {
      setError(String(err));
    }
  };

  onMount(async () => {
    await loadModels();
    area?.focus();
    const unlisten: UnlistenFn = await listen<string>("quick:delta", (event) => {
      setAnswer((prev) => prev + event.payload);
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void hide();
    };
    document.addEventListener("keydown", onKey);
    const unlistenFocus = await getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) {
        void loadModels();
        window.setTimeout(() => area?.focus(), 50);
      } else {
        void api.overlayHide().catch(() => undefined);
      }
    });
    onCleanup(() => {
      unlisten();
      document.removeEventListener("keydown", onKey);
      unlistenFocus();
    });
  });

  const hide = async () => {
    try {
      await api.overlayHide();
    } catch {
      await getCurrentWindow().hide();
    }
  };

  const submit = async () => {
    const text = prompt().trim();
    if (!text || busy()) return;
    if (!modelId()) {
      setError("Нет доступной модели");
      return;
    }
    setBusy(true);
    setError(null);
    setAnswer("");
    try {
      const result = await api.quickAsk(text, modelId());
      if (!answer()) setAnswer(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="quick-overlay" onKeyDown={(event) => {
      if (event.key === "Escape") void hide();
    }}>
      <header class="quick-head">
        <strong>Legamind · быстрый вопрос</strong>
        <div class="row">
          <select
            value={modelId()}
            onChange={(event) => setModelId(event.currentTarget.value)}
          >
            <Show when={localModels().length > 0} fallback={<option disabled>Локальные модели не найдены</option>}>
              <optgroup label="Локальные">
                <For each={localModels()}>
                  {(model) => <option value={model.id}>{model.label}</option>}
                </For>
              </optgroup>
            </Show>
            <Show when={cloudModels().length > 0}>
              <optgroup label="Облачные">
                <For each={cloudModels()}>
                  {(model) => (
                    <option value={model.id}>
                      {model.label} · {model.provider}
                    </option>
                  )}
                </For>
              </optgroup>
            </Show>
          </select>
          <button class="btn small ghost" onClick={() => void hide()} title="Закрыть (Esc)">
            ✕
          </button>
        </div>
      </header>
      <textarea
        ref={area}
        class="quick-input"
        rows={2}
        placeholder="Спросите что угодно… (Enter — отправить, Shift+Enter — перенос)"
        value={prompt()}
        onInput={(event) => setPrompt(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void submit();
          }
        }}
      />
      <div class="row">
        <button class="btn primary" onClick={() => void submit()} disabled={busy() || !prompt().trim()}>
          {busy() ? "Думаю…" : "Спросить"}
        </button>
        <span class="muted small">Ctrl+Shift+Space — показать/скрыть</span>
      </div>
      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={answer()}>
        <div class="quick-answer">{answer()}</div>
      </Show>
    </div>
  );
}
