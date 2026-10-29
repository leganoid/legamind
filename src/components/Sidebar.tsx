import { For, Show } from "solid-js";
import type { Session } from "../types";

interface Props {
  sessions: Session[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
}

export default function Sidebar(props: Props) {
  const rename = (session: Session) => {
    const title = window.prompt("Название сессии", session.title);
    if (title && title.trim() && title.trim() !== session.title) {
      props.onRename(session.id, title.trim());
    }
  };

  const remove = (session: Session) => {
    if (window.confirm(`Удалить сессию «${session.title}»?`)) {
      props.onDelete(session.id);
    }
  };

  return (
    <aside class="sidebar">
      <div class="sidebar-head">
        <span>Сессии</span>
        <button class="btn small" title="Новая сессия (Ctrl+N)" onClick={() => props.onCreate()}>
          + Новая
        </button>
      </div>
      <div class="session-list">
        <Show when={props.sessions.length === 0}>
          <p class="muted small pad">Пока нет сессий</p>
        </Show>
        <For each={props.sessions}>
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
    </aside>
  );
}
