import { For, Show, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { MemoryRow } from "../types";

const KIND_LABEL: Record<string, string> = {
  fact: "факт",
  episode: "эпизод",
  preference: "предпочтение",
};

export default function MemoryPanel() {
  const [items, setItems] = createSignal<MemoryRow[]>([]);
  const [query, setQuery] = createSignal("");
  const [kind, setKind] = createSignal("fact");
  const [text, setText] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [searching, setSearching] = createSignal(false);

  const load = async () => {
    setItems(await api.memoriesList());
  };

  const add = async () => {
    if (!text().trim()) return;
    try {
      await api.memoryAdd(kind(), text().trim());
      setText("");
      setError(null);
      await load();
    } catch (err) {
      setError(String(err));
    }
  };

  const search = async () => {
    if (!query().trim()) {
      setSearching(false);
      await load();
      return;
    }
    setSearching(true);
    setItems(await api.memorySearch(query().trim(), 50));
  };

  const remove = async (id: string) => {
    await api.memoryDelete(id);
    await load();
  };

  onMount(load);

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Память</h1>
          <p class="muted small">
            Факты и эпизоды подмешиваются в начало запроса (топ-5 по FTS5, бюджет ~800 токенов).
          </p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>

      <section class="card form">
        <div class="row">
          <select value={kind()} onChange={(e) => setKind(e.currentTarget.value)}>
            <option value="fact">Факт</option>
            <option value="preference">Предпочтение</option>
            <option value="episode">Эпизод</option>
          </select>
          <input
            class="grow"
            value={text()}
            placeholder="Например: в binres используется JFFS2"
            onInput={(e) => setText(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") add();
            }}
          />
          <button class="btn primary" onClick={add}>
            Добавить
          </button>
        </div>
        <div class="row">
          <input
            class="grow"
            value={query()}
            placeholder="Поиск по памяти…"
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") search();
            }}
          />
          <button class="btn" onClick={search}>
            Искать
          </button>
          <Show when={searching()}>
            <button
              class="btn ghost"
              onClick={() => {
                setQuery("");
                setSearching(false);
                load();
              }}
            >
              Сбросить
            </button>
          </Show>
        </div>
      </section>

      <Show when={items().length === 0}>
        <p class="empty-inline">Память пуста.</p>
      </Show>

      <div class="memory-list">
        <For each={items()}>
          {(item) => (
            <section class="card memory">
              <div>
                <span classList={{ badge: true, on: item.kind === "fact" }}>{KIND_LABEL[item.kind] ?? item.kind}</span>
                <span class="muted small"> {new Date(item.created_at).toLocaleString("ru-RU")}</span>
                <p class="memory-text">{item.text}</p>
              </div>
              <button class="icon-btn danger" title="Удалить" onClick={() => remove(item.id)}>
                ✕
              </button>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
