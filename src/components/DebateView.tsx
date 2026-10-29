import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  DebateDeltaPayload,
  DebateDonePayload,
  DebateErrorPayload,
  DebateRow,
  DebateTurnPayload,
  DebateTurnRow,
  ModelInfo,
  ScoreEntry,
} from "../types";
import DebateSetup, { type DebateLaunch } from "./DebateSetup";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  onModelsChanged: () => void;
}

const ROLE_LABEL: Record<string, string> = {
  proposal: "предложение",
  critique: "критика",
  vote: "голос",
  judge: "вердикт",
};

export default function DebateView(props: Props) {
  const [turns, setTurns] = createSignal<DebateTurnRow[]>([]);
  const [live, setLive] = createSignal<{ modelId: string; round: number; text: string } | null>(null);
  const [verdict, setVerdict] = createSignal<string | null>(null);
  const [finalAnswer, setFinalAnswer] = createSignal<string | null>(null);
  const [scores, setScores] = createSignal<ScoreEntry[]>([]);
  const [debateId, setDebateId] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [history, setHistory] = createSignal<DebateRow[]>([]);
  const [taskTitle, setTaskTitle] = createSignal<string>("");

  const modelLabel = (id: string) => props.models.find((m) => m.id === id)?.label ?? id;

  const grouped = createMemo(() => {
    const map = new Map<number, DebateTurnRow[]>();
    for (const turn of turns()) {
      const list = map.get(turn.round) ?? [];
      list.push(turn);
      map.set(turn.round, list);
    }
    return [...map.entries()].sort((a, b) => a[0] - b[0]);
  });

  const refreshHistory = async () => setHistory(await api.debatesList(30));

  const start = async (launch: DebateLaunch) => {
    setError(null);
    setNotice(null);
    setTurns([]);
    setVerdict(null);
    setFinalAnswer(null);
    setScores([]);
    setLive(null);
    setTaskTitle(launch.task);
    try {
      const result = await api.debateStart(launch);
      setDebateId(result.debate_id);
      setBusy(true);
    } catch (err) {
      setError(String(err));
    }
  };

  const stop = async () => {
    const id = debateId();
    if (id) await api.debateStop(id);
  };

  const exportMd = async () => {
    const id = debateId();
    if (!id) return;
    try {
      const path = await api.debateExportMd(id);
      setNotice(`Экспортировано: ${path}`);
    } catch (err) {
      setError(String(err));
    }
  };

  const loadDebate = async (id: string) => {
    const detail = await api.debateGet(id);
    setDebateId(id);
    setTaskTitle(detail.debate.task);
    setTurns(detail.turns);
    setVerdict(detail.debate.verdict);
    setFinalAnswer(detail.debate.final_answer);
    setScores([]);
    setLive(null);
  };

  onMount(async () => {
    await refreshHistory();
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await listen<DebateTurnPayload>("debate:turn", async (event) => {
        if (event.payload.debate_id !== debateId()) return;
        setTurns((prev) => [...prev, event.payload.turn]);
        setLive(null);
      }),
    );
    unlisteners.push(
      await listen<DebateDeltaPayload>("debate:delta", (event) => {
        const payload = event.payload;
        if (payload.debate_id !== debateId()) return;
        setLive((prev) => {
          if (!prev || prev.modelId !== payload.model_id || prev.round !== payload.round) {
            return { modelId: payload.model_id, round: payload.round, text: payload.delta };
          }
          return { ...prev, text: prev.text + payload.delta };
        });
      }),
    );
    unlisteners.push(
      await listen<DebateDonePayload>("debate:done", async (event) => {
        if (event.payload.debate_id !== debateId()) return;
        setVerdict(event.payload.verdict);
        setFinalAnswer(event.payload.final_answer);
        setScores(event.payload.scores);
        setLive(null);
        setBusy(false);
        await refreshHistory();
        props.onModelsChanged();
      }),
    );
    unlisteners.push(
      await listen<DebateErrorPayload>("debate:error", async (event) => {
        if (event.payload.debate_id !== debateId()) return;
        setError(event.payload.error);
        setLive(null);
        setBusy(false);
        await refreshHistory();
      }),
    );
    onCleanup(() => unlisteners.forEach((fn) => fn()));
  });

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Дебаты</h1>
          <p class="muted small">
            Участники получают анонимизированные реплики друг друга; итог — вердикт судьи или синтез
            победителя голосования.
          </p>
        </div>
        <div class="row">
          <select
            class="history-select"
            onChange={(e) => {
              const value = e.currentTarget.value;
              if (value) loadDebate(value);
            }}
          >
            <option value="">История дебатов…</option>
            <For each={history()}>
              {(item) => <option value={item.id}>{item.task.slice(0, 70)}</option>}
            </For>
          </select>
          <Show when={debateId()}>
            <button class="btn small" onClick={exportMd}>
              Экспорт .md
            </button>
          </Show>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">
          {error()}
          <button class="icon-btn" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">
          {notice()}
          <button class="icon-btn" onClick={() => setNotice(null)}>
            ✕
          </button>
        </div>
      </Show>

      <Show when={!debateId()}>
        <DebateSetup
          models={props.models}
          providerLabels={props.providerLabels}
          busy={busy()}
          onStart={start}
          onStop={stop}
        />
      </Show>

      <Show when={debateId()}>
        <section class="card debate-task">
          <div class="row space-between">
            <h2>Задача</h2>
            <button class="btn small ghost" onClick={() => setDebateId(null)}>
              Новые дебаты
            </button>
          </div>
          <p class="debate-task-text">{taskTitle()}</p>
          <Show when={busy()}>
            <p class="muted small">Идёт обсуждение… реплики появляются по мере готовности.</p>
          </Show>
        </section>

        <For each={grouped()}>
          {([round, items]) => (
            <section class="round-block">
              <h3 class="round-title">Раунд {round}</h3>
              <For each={items}>
                {(turn) => (
                  <article class="turn" classList={{ judge: turn.role === "judge", vote: turn.role === "vote" }}>
                    <header>
                      <span class="turn-author">{modelLabel(turn.model_id)}</span>
                      <span class="badge on">{ROLE_LABEL[turn.role] ?? turn.role}</span>
                      <Show when={turn.tokens}>
                        <span class="message-meta">{turn.tokens} ток.</span>
                      </Show>
                      <Show when={turn.seconds}>
                        <span class="message-meta">{turn.seconds!.toFixed(1)} с</span>
                      </Show>
                    </header>
                    <div class="message-body">{turn.content}</div>
                  </article>
                )}
              </For>
            </section>
          )}
        </For>

        <Show when={live()}>
          <article class="turn live">
            <header>
              <span class="turn-author">{modelLabel(live()!.modelId)}</span>
              <span class="badge">раунд {live()!.round}</span>
              <span class="message-meta streaming">печатает…</span>
            </header>
            <div class="message-body">{live()!.text}</div>
          </article>
        </Show>

        <Show when={scores().length > 0}>
          <section class="card">
            <h3>Голосование</h3>
            <ul class="score-list">
              <For each={scores()}>
                {(entry) => (
                  <li>
                    <span>{entry.label}</span>
                    <span class="score">{entry.score}</span>
                  </li>
                )}
              </For>
            </ul>
          </section>
        </Show>

        <Show when={verdict()}>
          <section class="card verdict">
            <h3>Вердикт</h3>
            <p class="message-body">{verdict()}</p>
            <Show when={finalAnswer() && finalAnswer() !== verdict()}>
              <h3>Итоговое решение</h3>
              <p class="message-body">{finalAnswer()}</p>
            </Show>
          </section>
        </Show>
      </Show>
    </div>
  );
}
