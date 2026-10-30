import { For, Show, createSignal } from "solid-js";
import { api } from "../api";
import type { MessageHitRow, Session } from "../types";
import ConfirmDialog from "./ConfirmDialog";

interface Props {
  sessions: Session[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
}

export default function Sidebar(props: Props) {
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<MessageHitRow[]>([]);
  const [pendingDelete, setPendingDelete] = createSignal<Session | null>(null);
  let timer: number | undefined;

  const filtered = () => {
    const q = query().trim().toLowerCase();
    if (!q) return props.sessions;
    return props.sessions.filter((session) => session.title.toLowerCase().includes(q));
  };

  const onSearch = (value: string) => {
    setQuery(value);
    window.clearTimeout(timer);
    const q = value.trim();
    if (q.length < 2) {
      setHits([]);
      return;
    }
    timer = window.setTimeout(async () => {
      try {
        setHits(await api.messagesSearch(q, 15));
      } catch {
        setHits([]);
      }
    }, 300);
  };

  const rename = (session: Session) => {
    const title = window.prompt("Название сессии", session.title);
    if (title && title.trim() && title.trim() !== session.title) {
      props.onRename(session.id, title.trim());
    }
  };

  const remove = (session: Session) => {
    setPendingDelete(session);
  };

  return (
    <aside class="sidebar">
      <div class="sidebar-head">
        <span>Сессии</span>
        <button class="btn small" title="Новая сессия (Ctrl+N)" onClick={() => props.onCreate()}>
          + Новая
        </button>
      </div>

      <div class="sidebar-search">
        <input
          value={query()}
          placeholder="Поиск по сессиям и сообщениям"
          onInput={(event) => onSearch(event.currentTarget.value)}
        />
      </div>

      <Show when={query().trim().length >= 2 && hits().length > 0}>
        <div class="search-results">
          <div class="muted small pad">Найдено в сообщениях</div>
          <For each={hits()}>
            {(hit) => (
              <button class="search-hit" onClick={() => props.onSelect(hit.session_id)}>
                <span class="search-hit-title">{hit.session_title}</span>
                <span class="search-hit-snippet">{hit.snippet}</span>
              </button>
            )}
          </For>
        </div>
      </Show>

      <div class="session-list">
        <Show when={props.sessions.length === 0}>
          <p class="muted small pad">Пока нет сессий</p>
        </Show>
        <Show when={props.sessions.length > 0 && filtered().length === 0}>
          <p class="muted small pad">Сессии не найдены</p>
        </Show>
        <For each={filtered()}>
          {(session) => (
            <div
              classList={{ "session-item": true, active: session.id === props.activeId }}
              onClick={() => props.onSelect(session.id)}
            >
              <div class="session-title" title={session.title}>
                {session.title}
              </div>
              <div class="session-actions">
                <button
                  class="icon-btn"
                  title="Переименовать"
                  onClick={(e) => {
                    e.stopPropagation();
                    rename(session);
                  }}
                >
                  ✎
                </button>
                <button
                  class="icon-btn danger"
                  title="Удалить"
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(session);
                  }}
                >
                  ✕
                </button>
              </div>
            </div>
          )}
        </For>
      </div>

      <ConfirmDialog
        open={pendingDelete() !== null}
        title="Удалить сессию?"
        message={`«${pendingDelete()?.title ?? ""}» и её сообщения будут удалены. Это действие нельзя отменить.`}
        confirmLabel="Удалить"
        danger
        onConfirm={() => {
          const session = pendingDelete();
          setPendingDelete(null);
          if (session) props.onDelete(session.id);
        }}
        onCancel={() => setPendingDelete(null)}
      />
    </aside>
  );
}
