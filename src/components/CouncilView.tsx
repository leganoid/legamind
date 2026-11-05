import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type { CouncilAnswer, ModelInfo } from "../types";

interface Props {
  models: ModelInfo[];
}

interface Card {
  model: string;
  text: string;
  busy: boolean;
  error: string | null;
  tokens: number;
  seconds: number;
}

export default function CouncilView(props: Props) {
  const chatModels = createMemo(() => props.models.filter((model) => model.role === "chat"));
  const [selected, setSelected] = createSignal<string[]>([]);
  const [prompt, setPrompt] = createSignal("");
  const [cards, setCards] = createSignal<Card[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let initialized = false;
  let unlisten: UnlistenFn[] = [];

  createEffect(() => {
    if (!initialized && chatModels().length > 0) {
      initialized = true;
      setSelected(chatModels().map((model) => model.id));
    }
  });

  onMount(async () => {
    unlisten.push(
      await listen<{ model: string; delta: string }>("council:delta", (event) => {
        setCards((prev) =>
          prev.map((card) =>
            card.model === event.payload.model
              ? { ...card, text: card.text + event.payload.delta }
              : card,
          ),
        );
      }),
    );
    unlisten.push(
      await listen<CouncilAnswer>("council:done", (event) => {
        const answer = event.payload;
        setCards((prev) =>
          prev.map((card) =>
            card.model === answer.model
              ? {
                  ...card,
                  text: answer.text || card.text,
                  busy: false,
                  error: answer.error,
                  tokens: answer.tokens,
                  seconds: answer.seconds,
                }
              : card,
          ),
        );
      }),
    );
  });
  onCleanup(() => unlisten.forEach((fn) => fn()));

  const toggle = (id: string) => {
    setSelected((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id],
    );
  };

  const ask = async () => {
    const text = prompt().trim();
    if (!text || selected().length === 0) return;
    setBusy(true);
    setError(null);
    setCards(
      selected().map((model) => ({
        model,
        text: "",
        busy: true,
        error: null,
        tokens: 0,
        seconds: 0,
      })),
    );
    try {
      const answers = await api.councilAsk(text, selected());
      setCards(
        answers.map((answer) => ({
          model: answer.model,
          text: answer.text,
          busy: false,
          error: answer.error,
          tokens: answer.tokens,
          seconds: answer.seconds,
        })),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Совет</h1>
          <p class="muted small">
            Одна задача — ответы всех выбранных моделей. Локальные модели отвечают
            последовательно, чтобы не переполнить память.
          </p>
        </div>
      </header>

      <section class="card">
        <div class="council-models">
          <For each={chatModels()}>
            {(model) => (
              <label class="council-check">
                <input
                  type="checkbox"
                  checked={selected().includes(model.id)}
                  onChange={() => toggle(model.id)}
                />
                <span>{model.label}</span>
                <span class="muted small">{model.provider}</span>
              </label>
            )}
          </For>
        </div>
        <textarea
          rows={3}
          value={prompt()}
          placeholder="Задача для всех моделей…"
          onInput={(event) => setPrompt(event.currentTarget.value)}
        />
        <button
          class="btn primary"
          onClick={() => void ask()}
          disabled={busy() || !prompt().trim() || selected().length === 0}
        >
          {busy() ? "Совет отвечает…" : `Спросить все (${selected().length})`}
        </button>
      </section>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>

      <div class="council-grid">
        <For each={cards()}>
          {(card) => (
            <section class="card council-card">
              <header class="council-card-head">
                <strong>{chatModels().find((model) => model.id === card.model)?.label ?? card.model}</strong>
                <span class="muted small">
                  {card.busy
                    ? "думает…"
                    : card.error
                      ? "ошибка"
                      : `${card.tokens} ток. · ${card.seconds.toFixed(1)} с`}
                </span>
              </header>
              <Show when={card.error}>
                <div class="banner error">{card.error}</div>
              </Show>
              <div class="council-answer">{card.text || (card.busy ? "…" : "")}</div>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
