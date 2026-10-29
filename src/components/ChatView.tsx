import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  ChatDeltaPayload,
  ChatDonePayload,
  ChatErrorPayload,
  ChatMessageRow,
  ModelInfo,
  Session,
} from "../types";
import Sidebar from "./Sidebar";
import ChatMessage from "./Message";
import Composer from "./Composer";
import ModelPicker from "./ModelPicker";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  onModelsChanged: () => void;
}

const MODEL_STORAGE_KEY = "legamind.lastModel";

export default function ChatView(props: Props) {
  const [sessions, setSessions] = createSignal<Session[]>([]);
  const [activeId, setActiveId] = createSignal<string | null>(null);
  const [messages, setMessages] = createSignal<ChatMessageRow[]>([]);
  const [modelId, setModelId] = createSignal("");
  const [requestId, setRequestId] = createSignal<string | null>(null);
  const [streamText, setStreamText] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  let scroller: HTMLDivElement | undefined;

  const busy = () => requestId() !== null;
  const activeSession = createMemo(() => sessions().find((s) => s.id === activeId()) ?? null);

  const modelLabel = (id: string | null) => {
    if (!id) return "Ассистент";
    const found = props.models.find((m) => m.id === id);
    return found?.label ?? id;
  };

  const defaultModel = () => {
    const stored = localStorage.getItem(MODEL_STORAGE_KEY);
    if (stored && props.models.some((m) => m.id === stored)) return stored;
    const preferred = props.models.find((m) => m.id === "local/qwen3-14b");
    const local = props.models.find((m) => m.kind === "local");
    return preferred?.id ?? local?.id ?? props.models[0]?.id ?? "";
  };

  const refreshSessions = async () => {
    setSessions(await api.sessionsList());
  };

  const openSession = async (id: string) => {
    setActiveId(id);
    setError(null);
    setMessages(await api.messagesList(id));
    const session = sessions().find((s) => s.id === id);
    if (session && props.models.some((m) => m.id === session.model_id)) {
      setModelId(session.model_id);
    }
  };

  const createSession = async () => {
    const chosen = modelId() || defaultModel();
    if (!chosen) {
      setError("Нет доступных моделей");
      return;
    }
    const session = await api.sessionsCreate(chosen);
    setSessions([session, ...sessions()]);
    setActiveId(session.id);
    setMessages([]);
    setError(null);
  };

  const renameSession = async (id: string, title: string) => {
    await api.sessionsRename(id, title);
    await refreshSessions();
  };

  const deleteSession = async (id: string) => {
    await api.sessionsDelete(id);
    const rest = sessions().filter((s) => s.id !== id);
    setSessions(rest);
    if (activeId() === id) {
      if (rest.length > 0) {
        await openSession(rest[0].id);
      } else {
        setActiveId(null);
        setMessages([]);
      }
    }
  };

  const send = async (text: string) => {
    setError(null);
    setNotice(null);
    let session = activeSession();
    if (!session) {
      const chosen = modelId() || defaultModel();
      if (!chosen) {
        setError("Выберите модель и создайте сессию");
        return;
      }
      session = await api.sessionsCreate(chosen);
      setSessions((prev) => [session!, ...prev]);
      setActiveId(session.id);
      setMessages([]);
    }
    const chosen = modelId() || defaultModel();
    try {
      const start = await api.chatSend(session.id, text, chosen);
      setMessages((prev) => [...prev, start.message]);
      setRequestId(start.request_id);
      setStreamText("");
      if (session.title === "Новая сессия") {
        const title = text.length > 48 ? text.slice(0, 48) + "…" : text;
        await api.sessionsRename(session.id, title);
        await refreshSessions();
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const stop = async () => {
    const id = requestId();
    if (id) {
      await api.chatStop(id);
    }
  };

  onMount(async () => {
    setModelId(defaultModel());
    await refreshSessions();
    const list = await api.sessionsList();
    if (list.length > 0) {
      await openSession(list[0].id);
    }

    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await listen<ChatDeltaPayload>("chat:delta", (event) => {
        if (event.payload.session_id !== activeId()) return;
        setStreamText((prev) => prev + event.payload.delta);
      }),
    );
    unlisteners.push(
      await listen<ChatDonePayload>("chat:done", async (event) => {
        if (event.payload.session_id !== activeId()) {
          await refreshSessions();
          return;
        }
        setMessages((prev) => [...prev, event.payload.message]);
        setRequestId(null);
        setStreamText("");
        if (event.payload.stopped) setNotice("Генерация остановлена");
        await refreshSessions();
        props.onModelsChanged();
      }),
    );
    unlisteners.push(
      await listen<ChatErrorPayload>("chat:error", (event) => {
        if (event.payload.session_id !== activeId()) return;
        setError(event.payload.error);
        setRequestId(null);
        setStreamText("");
      }),
    );

    // Ctrl+N из App: создать новую сессию.
    const newSession = () => createSession();
    window.addEventListener("legamind:new-session", newSession);
    onCleanup(() => window.removeEventListener("legamind:new-session", newSession));
    onCleanup(() => unlisteners.forEach((fn) => fn()));
  });

  // Модели приходят асинхронно: выбираем модель, когда список появился.
  createEffect(() => {
    const list = props.models;
    if (list.length === 0) return;
    const current = modelId();
    if (!current || !list.some((m) => m.id === current)) {
      setModelId(defaultModel());
    }
  });

  // Автопрокрутка вниз при новых сообщениях и дельтах.
  createEffect(() => {
    messages();
    streamText();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  });

  const chooseModel = (id: string) => {
    setModelId(id);
    localStorage.setItem(MODEL_STORAGE_KEY, id);
  };

  return (
    <div class="chat-layout">
      <Sidebar
        sessions={sessions()}
        activeId={activeId()}
        onSelect={(id) => openSession(id)}
        onCreate={createSession}
        onRename={renameSession}
        onDelete={deleteSession}
      />
      <section class="chat-main">
        <header class="chat-head">
          <ModelPicker
            models={props.models}
            providerLabels={props.providerLabels}
            value={modelId()}
            onChange={chooseModel}
            disabled={busy()}
          />
          <span class="muted small">
            {activeSession() ? activeSession()!.title : "Сессия не выбрана"}
          </span>
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

        <div class="messages" ref={scroller}>
          <Show when={messages().length === 0 && !busy()}>
            <div class="empty">
              <h2>Legamind</h2>
              <p class="muted">
                Локальные модели OpenVINO и облачные провайдеры в одном окне. Начните диалог или
                создайте новую сессию.
              </p>
              <p class="muted small">Подсказка: «запомни: …» сохраняет факт в память.</p>
            </div>
          </Show>
          <For each={messages()}>
            {(message) => <ChatMessage message={message} modelLabel={modelLabel} />}
          </For>
          <Show when={busy()}>
            <ChatMessage
              message={{
                id: "streaming",
                session_id: activeId() ?? "",
                role: "assistant",
                content: streamText() || "…",
                model_id: modelId(),
                tokens: null,
                created_at: new Date().toISOString(),
              }}
              modelLabel={modelLabel}
              streaming
            />
          </Show>
        </div>

        <Composer busy={busy()} disabled={false} onSend={send} onStop={stop} />
      </section>
    </div>
  );
}
