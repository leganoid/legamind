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
  const [arbiter, setArbiter] = createSignal("");
  const [summary, setSummary] = createSignal<Card | null>(null);
  let initialized = false;
  let unlisten: UnlistenFn[] = [];

  createEffect(() => {
    if (!initialized && chatModels().length > 0) {
      initialized = true;
      setSelected(chatModels().map((model) => model.id));
      const smart = chatModels().find((model) => model.id.toLowerCase().includes("14b"));
      setArbiter(smart?.id ?? chatModels()[0]?.id ?? "");
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
    unlisten.push(
      await listen<{ model: string; delta: string }>("council:summary:delta", (event) => {
        setSummary((prev) => ({
          model: event.payload.model,
          text: (prev?.text ?? "") + event.payload.delta,
          busy: true,
          error: null,
          tokens: 0,
          seconds: 0,
        }));
      }),
    );
    unlisten.push(
      await listen<CouncilAnswer>("council:summary_done", (event) => {
        const answer = event.payload;
        setSummary({
          model: answer.model,
          text: answer.text,
          busy: false,
          error: answer.error,
          tokens: answer.tokens,
          seconds: answer.seconds,
        });
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
    setSummary(null);
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
      const outcome = await api.councilAsk(text, selected(), arbiter() || undefined);
      setCards(
        outcome.answers.map((answer) => ({
          model: answer.model,
          text: answer.text,
          busy: false,
          error: answer.error,
          tokens: answer.tokens,
          seconds: answer.seconds,
        })),
      );
      if (outcome.summary) {
        setSummary({
          model: outcome.summary.model,
          text: outcome.summary.text,
          busy: false,
          error: outcome.summary.error,
          tokens: outcome.summary.tokens,
          seconds: outcome.summary.seconds,
        });
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    try {
      await api.councilCancel();
    } catch (err) {
      setError(String(err));
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
        <div class="row">
          <label class="field">
            <span>Арбитр сводки</span>
            <select
              value={arbiter()}
              onChange={(event) => setArbiter(event.currentTarget.value)}
            >
              <option value="">Без сводки</option>
              <For each={chatModels()}>
                {(model) => <option value={model.id}>{model.label}</option>}
              </For>
            </select>
          </label>
          <button
            class="btn primary"
            onClick={() => void ask()}
            disabled={busy() || !prompt().trim() || selected().length === 0}
          >
            {busy() ? "Совет отвечает…" : `Спросить все (${selected().length})`}
          </button>
          <Show when={busy()}>
            <button class="btn ghost" onClick={() => void stop()}>
              Стоп
            </button>
          </Show>
        </div>
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

      <Show when={summary()}>
        {(card) => (
          <section class="card council-summary">
            <header class="council-card-head">
              <strong>Сводка арбитра</strong>
              <span class="muted small">
                {chatModels().find((model) => model.id === card().model)?.label ?? card().model}
                {" · "}
                {card().busy
                  ? "думает…"
                  : card().error
                    ? "ошибка"
                    : `${card().tokens} ток. · ${card().seconds.toFixed(1)} с`}
              </span>
            </header>
            <Show when={card().error}>
              <div class="banner error">{card().error}</div>
            </Show>
            <div class="council-answer">{card().text || (card().busy ? "…" : "")}</div>
          </section>
        )}
      </Show>
    </div>
  );
}
