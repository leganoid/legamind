import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  ChatDeltaPayload,
  ChatDonePayload,
  ChatErrorPayload,
  ChatMessageRow,
  ChatStatusPayload,
  ContextUsage,
  ModelInfo,
  Session,
  SourceRow,
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

function formatTokens(value: number): string {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k`;
  return String(value);
}

export default function ChatView(props: Props) {
  const [sessions, setSessions] = createSignal<Session[]>([]);
  const [activeId, setActiveId] = createSignal<string | null>(null);
  const [messages, setMessages] = createSignal<ChatMessageRow[]>([]);
  const [modelId, setModelId] = createSignal("");
  const [requestId, setRequestId] = createSignal<string | null>(null);
  const [streamText, setStreamText] = createSignal("");
  const [statusText, setStatusText] = createSignal<string | null>(null);
  const [commandBusy, setCommandBusy] = createSignal(false);
  const [webEnabled, setWebEnabled] = createSignal(true);
  const [sourcesByMessage, setSourcesByMessage] = createSignal<Record<string, SourceRow[]>>({});
  const [context, setContext] = createSignal<ContextUsage | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editText, setEditText] = createSignal("");
  let scroller: HTMLDivElement | undefined;

  const busy = () => requestId() !== null;
  const activeSession = createMemo(() => sessions().find((s) => s.id === activeId()) ?? null);

  const lastUserId = createMemo(() => {
    const list = messages().filter((m) => m.role === "user");
    return list.length > 0 ? list[list.length - 1].id : null;
  });
  const lastAssistantId = createMemo(() => {
    const list = messages().filter((m) => m.role === "assistant");
    return list.length > 0 ? list[list.length - 1].id : null;
  });

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

  const refreshContext = async () => {
    const id = activeId();
    const model = modelId();
    if (!id || !model) {
      setContext(null);
      return;
    }
    try {
      setContext(await api.contextUsage(id, model));
    } catch {
      setContext(null);
    }
  };

  const loadSources = async (id: string) => {
    try {
      const list = await api.sourcesList(id);
      const grouped: Record<string, SourceRow[]> = {};
      for (const source of list) {
        (grouped[source.message_id] ??= []).push(source);
      }
      setSourcesByMessage(grouped);
    } catch {
      setSourcesByMessage({});
    }
  };

  const toggleWeb = async () => {
    try {
      const config = await api.webConfigSet({ enabled: !webEnabled() });
      setWebEnabled(config.enabled);
      setNotice(
        config.enabled
          ? "Интернет включён: перед ответом модель получит результаты поиска"
          : "Интернет выключен",
      );
    } catch (err) {
      setError(String(err));
    }
  };

  const openSession = async (id: string) => {
    setActiveId(id);
    setError(null);
    setMessages(await api.messagesList(id));
    await loadSources(id);
    const session = sessions().find((s) => s.id === id);
    if (session && props.models.some((m) => m.id === session.model_id)) {
      setModelId(session.model_id);
    }
    await refreshContext();
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
    setContext(null);
    setError(null);
  };

  const renameSession = async (id: string, title: string) => {
    await api.sessionsRename(id, title);
    await refreshSessions();
  };

  const deleteSession = async (id: string) => {
    const rid = requestId();
    if (rid && activeId() === id) await api.chatStop(rid);
    await api.sessionsDelete(id);
    const rest = sessions().filter((s) => s.id !== id);
    setSessions(rest);
    if (activeId() === id) {
      if (rest.length > 0) {
        await openSession(rest[0].id);
      } else {
        setActiveId(null);
        setMessages([]);
        setContext(null);
      }
    }
  };

  const beginStream = (id: string, removed: string[] = []) => {
    if (removed.length > 0) {
      setMessages((prev) => prev.filter((m) => !removed.includes(m.id)));
    }
    setRequestId(id);
    setStreamText("");
    setStatusText(null);
    setError(null);
  };

  const runCommand = async (raw: string) => {
    const id = activeId();
    if (!id) {
      setError("Сначала создайте сессию");
      return;
    }
    const chosen = modelId() || defaultModel();
    setCommandBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = await api.chatCommand(id, chosen, raw);
      if (result.kind === "clear") {
        setMessages([]);
        setContext(null);
        setNotice(result.text);
      } else if (result.kind === "compact") {
        setMessages(result.message ? [result.message] : []);
        setNotice(result.text);
        await refreshContext();
      } else {
        setNotice(result.text);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setCommandBusy(false);
    }
  };

  const send = async (text: string) => {
    const trimmed = text.trim();
    if (trimmed.startsWith("/")) {
      await runCommand(trimmed);
      return;
    }
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
      beginStream(start.request_id);
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
    if (id) await api.chatStop(id);
  };

  const regenerate = async () => {
    const id = activeId();
    if (!id || busy()) return;
    const chosen = modelId() || defaultModel();
    try {
      const start = await api.chatRegenerate(id, chosen);
      beginStream(start.request_id, start.removed_ids);
    } catch (err) {
      setError(String(err));
    }
  };

  const rate = async (message: ChatMessageRow, rating: number) => {
    let note: string | null = null;
    if (rating < 0) {
      const answer = window.prompt(
        "Что было не так? Правило сохранится и будет учитываться в ответах (можно оставить пустым):",
        "",
      );
      if (answer === null) return;
      note = answer.trim() || null;
    }
    try {
      await api.feedbackSet(message.id, message.session_id, rating, note);
      setNotice(
        rating > 0
          ? "Спасибо, оценка сохранена"
          : note
            ? "Правило сохранено — смотрите экран «Обучение»"
            : "Оценка сохранена",
      );
    } catch (err) {
      setError(String(err));
    }
  };

  const startEdit = (message: ChatMessageRow) => {
    setEditingId(message.id);
    setEditText(message.content);
    setError(null);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditText("");
  };

  const saveEdit = async () => {
    const id = activeId();
    const messageId = editingId();
    if (!id || !messageId) return;
    const text = editText().trim();
    if (!text) return;
    const chosen = modelId() || defaultModel();
    try {
      const start = await api.chatEditAndResend(id, messageId, text, chosen);
      setMessages((prev) => [...prev.filter((m) => !start.removed_ids.includes(m.id)), start.message]);
      beginStream(start.request_id, []);
      cancelEdit();
    } catch (err) {
      setError(String(err));
    }
  };

  onMount(async () => {
    setModelId(defaultModel());
    try {
      const config = await api.webConfigGet();
      setWebEnabled(config.enabled);
    } catch {
      // Конфиг интернета недоступен — остаётся значение по умолчанию.
    }
    await refreshSessions();
    const list = await api.sessionsList();
    if (list.length > 0) {
      await openSession(list[0].id);
    }

    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await listen<ChatDeltaPayload>("chat:delta", (event) => {
        if (event.payload.session_id !== activeId()) return;
        setStatusText(null);
        setStreamText((prev) => prev + event.payload.delta);
      }),
    );
    unlisteners.push(
      await listen<ChatStatusPayload>("chat:status", (event) => {
        if (event.payload.session_id !== activeId()) return;
        const { phase, label, device, seconds } = event.payload;
        if (phase === "loading") {
          setStatusText(`Загружаю ${label ?? "модель"}${device ? ` на ${device}` : ""}…`);
        } else if (phase === "ready") {
          setStatusText(
            `Модель готова${seconds ? ` за ${seconds.toFixed(1)} с` : ""}, генерирую…`,
          );
        } else if (phase === "connecting") {
          setStatusText(`Подключаюсь к ${label ?? "провайдеру"}…`);
        } else {
          setStatusText(null);
        }
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
        setStatusText(null);
        if (event.payload.context) setContext(event.payload.context);
        if (event.payload.stopped) setNotice("Генерация остановлена");
        await loadSources(event.payload.session_id);
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
        setStatusText(null);
      }),
    );

    // Ctrl+N из App: создать новую сессию.
    const newSession = () => createSession();
    window.addEventListener("legamind:new-session", newSession);
    onCleanup(() => window.removeEventListener("legamind:new-session", newSession));
    onCleanup(() => unlisteners.forEach((fn) => fn()));
  });

  // Автопрокрутка вниз при новых сообщениях и дельтах.
  createEffect(() => {
    messages();
    streamText();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
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

  // Индикатор контекста пересчитываем после каждого изменения истории.
  let contextTimer: number | undefined;
  createEffect(() => {
    activeId();
    modelId();
    messages().length;
    if (busy()) return;
    if (contextTimer) window.clearTimeout(contextTimer);
    contextTimer = window.setTimeout(() => {
      void refreshContext();
    }, 250);
  });
  onCleanup(() => {
    if (contextTimer) window.clearTimeout(contextTimer);
  });

  const chooseModel = (id: string) => {
    setModelId(id);
    localStorage.setItem(MODEL_STORAGE_KEY, id);
  };

  const contextTitle = () => {
    const usage = context();
    if (!usage) return "";
    return `Контекст: ${usage.used} из ${usage.limit} токенов, под ответ оставлено ${usage.reserve}. Обрезано сообщений: ${usage.dropped}.`;
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
          <Show when={context()}>
            <span
              class="context-chip"
              classList={{ warn: (context()!.used + context()!.reserve) / context()!.limit > 0.8 }}
              title={contextTitle()}
            >
              {formatTokens(context()!.used)} / {formatTokens(context()!.limit)}
            </span>
          </Show>
          <button
            classList={{ btn: true, small: true, active: webEnabled() }}
            title="Интернет: поиск перед ответом и источники"
            onClick={toggleWeb}
          >
            🌐 {webEnabled() ? "онлайн" : "офлайн"}
          </button>
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
        <Show when={commandBusy()}>
          <div class="banner info">Выполняю команду…</div>
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
            {(message) => (
              <ChatMessage
                message={message}
                modelLabel={modelLabel}
                sources={sourcesByMessage()[message.id]}
                actions={
                  !busy() && editingId() === null ? (
                    <>
                      <Show when={message.role === "assistant"}>
                        <button class="icon-btn" title="Полезный ответ" onClick={() => rate(message, 1)}>
                          👍
                        </button>
                        <button
                          class="icon-btn"
                          title="Плохой ответ — можно превратить в правило"
                          onClick={() => rate(message, -1)}
                        >
                          👎
                        </button>
                      </Show>
                      <Show when={message.id === lastAssistantId()}>
                        <button
                          class="btn tiny"
                          title="Удалить ответ и сгенерировать заново"
                          onClick={regenerate}
                        >
                          Перегенерировать
                        </button>
                      </Show>
                      <Show when={message.id === lastUserId()}>
                        <button
                          class="btn tiny"
                          title="Изменить сообщение и отправить заново"
                          onClick={() => startEdit(message)}
                        >
                          Изменить
                        </button>
                      </Show>
                    </>
                  ) : undefined
                }
              />
            )}
          </For>
          <Show when={busy()}>
            <ChatMessage
              message={{
                id: "streaming",
                session_id: activeId() ?? "",
                role: "assistant",
                content: streamText() || statusText() || "…",
                model_id: modelId(),
                tokens: null,
                created_at: new Date().toISOString(),
              }}
              modelLabel={modelLabel}
              streaming
            />
          </Show>
        </div>

        <Show
          when={editingId() !== null}
          fallback={
            <Composer busy={busy()} disabled={commandBusy()} onSend={send} onStop={stop} />
          }
        >
          <div class="composer edit-composer">
            <textarea
              rows={3}
              value={editText()}
              onInput={(e) => setEditText(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void saveEdit();
                }
                if (e.key === "Escape") cancelEdit();
              }}
            />
            <div class="composer-actions">
              <button class="btn primary" onClick={() => void saveEdit()} disabled={!editText().trim()}>
                Сохранить и отправить
              </button>
              <button class="btn ghost" onClick={cancelEdit}>
                Отмена
              </button>
            </div>
          </div>
        </Show>
      </section>
    </div>
  );
}
