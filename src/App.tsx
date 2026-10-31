import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { api } from "./api";
import type { ModelInfo, ProviderView, Session } from "./types";
import ChatView from "./components/ChatView";
import DebateView from "./components/DebateView";
import ModelsView from "./components/ModelsView";
import Providers from "./components/Providers";
import MemoryPanel from "./components/MemoryPanel";
import SettingsView from "./components/SettingsView";
import SkillsView from "./components/SkillsView";
import LearningView from "./components/LearningView";
import HelpView from "./components/HelpView";

type View =
  | "chat"
  | "debate"
  | "models"
  | "providers"
  | "memory"
  | "skills"
  | "learning"
  | "settings"
  | "help";

const NAV: { key: View; label: string; icon: string }[] = [
  { key: "chat", label: "Чат", icon: "💬" },
  { key: "debate", label: "Дебаты", icon: "⚖" },
  { key: "models", label: "Модели", icon: "🧩" },
  { key: "providers", label: "Провайдеры", icon: "🔑" },
  { key: "memory", label: "Память", icon: "🧠" },
  { key: "skills", label: "Скиллы", icon: "🧰" },
  { key: "learning", label: "Обучение", icon: "🎓" },
  { key: "settings", label: "Настройки", icon: "⚙" },
  { key: "help", label: "Помощь", icon: "❓" },
];

interface PaletteCommand {
  id: string;
  label: string;
  hint: string;
  icon: string;
  run: () => void;
}

export default function App() {
  const [view, setView] = createSignal<View>("chat");
  const [models, setModels] = createSignal<ModelInfo[]>([]);
  const [providers, setProviders] = createSignal<ProviderView[]>([]);
  const [error, setError] = createSignal<string | null>(null);

  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [paletteQuery, setPaletteQuery] = createSignal("");
  const [paletteIndex, setPaletteIndex] = createSignal(0);
  const [paletteSessions, setPaletteSessions] = createSignal<Session[]>([]);

  const providerLabels = createMemo(() => {
    const map: Record<string, string> = { local: "Локальные" };
    for (const provider of providers()) map[provider.id] = provider.label;
    return map;
  });

  const refreshModels = async () => {
    try {
      setModels(await api.modelsList());
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshProviders = async () => {
    setProviders(await api.providersList());
    await refreshModels();
  };

  const closePalette = () => {
    setPaletteOpen(false);
    setPaletteQuery("");
    setPaletteIndex(0);
  };

  const openPalette = () => {
    setPaletteOpen(true);
    setPaletteQuery("");
    setPaletteIndex(0);
  };

  const commands = (): PaletteCommand[] => [
    ...NAV.map((item) => ({
      id: item.key,
      label: item.label,
      hint: "Раздел",
      icon: item.icon,
      run: () => {
        setView(item.key);
        closePalette();
      },
    })),
    {
      id: "new-chat",
      label: "Новая сессия",
      hint: "Ctrl+N",
      icon: "＋",
      run: () => {
        setView("chat");
        window.dispatchEvent(new Event("legamind:new-session"));
        closePalette();
      },
    },
    ...paletteSessions()
      .slice(0, 10)
      .map((session) => ({
        id: `session-${session.id}`,
        label: session.title,
        hint: "Сессия",
        icon: "💬",
        run: () => {
          setView("chat");
          window.setTimeout(() => {
            window.dispatchEvent(
              new CustomEvent("legamind:open-session", { detail: session.id }),
            );
          }, 80);
          closePalette();
        },
      })),
    {
      id: "data-dir",
      label: "Открыть папку данных",
      hint: "БД и логи",
      icon: "📂",
      run: () => {
        void api.openDataDir();
        closePalette();
      },
    },
    ...(view() === "chat"
      ? [
          {
            id: "project-index",
            label: "Проиндексировать проект",
            hint: "RAG по рабочей папке",
            icon: "📚",
            run: () => {
              window.dispatchEvent(new Event("legamind:project-index"));
              closePalette();
            },
          },
          {
            id: "tool-history",
            label: "История инструментов",
            hint: "Проект",
            icon: "🧰",
            run: () => {
              window.dispatchEvent(new Event("legamind:tool-history"));
              closePalette();
            },
          },
          {
            id: "rag-toggle",
            label: "Переключить RAG",
            hint: "Поиск по проекту",
            icon: "🔎",
            run: () => {
              window.dispatchEvent(new Event("legamind:rag-toggle"));
              closePalette();
            },
          },
        ]
      : []),
  ];

  const filteredCommands = () => {
    const query = paletteQuery().trim().toLowerCase();
    const items = commands();
    if (!query) return items;
    return items.filter((item) =>
      `${item.label} ${item.hint}`.toLowerCase().includes(query),
    );
  };

  const runActive = () => {
    const list = filteredCommands();
    if (list.length === 0) return;
    list[paletteIndex() % list.length].run();
  };

  onMount(async () => {
    await refreshProviders();
    const timer = window.setInterval(refreshModels, 15000);
    window.addEventListener("keydown", (event) => {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setView("chat");
        window.dispatchEvent(new Event("legamind:new-session"));
      }
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (paletteOpen()) closePalette();
        else openPalette();
      }
      if (event.key === "Escape" && paletteOpen()) closePalette();
    });
    window.addEventListener("beforeunload", () => window.clearInterval(timer));
    window.addEventListener("legamind:sessions", (event) => {
      const detail = (event as CustomEvent<Session[]>).detail;
      if (Array.isArray(detail)) setPaletteSessions(detail);
    });
  });

  return (
    <div class="app">
      <nav class="nav">
        <div class="brand">
          <span class="brand-mark">L</span>
          <span>Legamind</span>
        </div>
        <div class="nav-items">
          {NAV.map((item) => (
            <button
              classList={{ "nav-item": true, active: view() === item.key }}
              onClick={() => setView(item.key)}
            >
              <span class="nav-icon">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </div>
        <div class="nav-foot muted small">
          <Show when={models().filter((m) => m.kind === "local").length > 0} fallback="Движок не отвечает">
            Локальных моделей: {models().filter((m) => m.kind === "local").length}
          </Show>
        </div>
      </nav>

      <main class="main">
        <Show when={error()}>
          <div class="banner error">{error()}</div>
        </Show>
        <Show when={view() === "chat"}>
          <ChatView models={models()} providerLabels={providerLabels()} onModelsChanged={refreshModels} />
        </Show>
        <Show when={view() === "debate"}>
          <DebateView models={models()} providerLabels={providerLabels()} onModelsChanged={refreshModels} />
        </Show>
        <Show when={view() === "models"}>
          <ModelsView models={models()} providerLabels={providerLabels()} onModelsChanged={refreshModels} />
        </Show>
        <Show when={view() === "providers"}>
          <Providers providers={providers()} onChanged={refreshProviders} />
        </Show>
        <Show when={view() === "memory"}>
          <MemoryPanel />
        </Show>
        <Show when={view() === "skills"}>
          <SkillsView />
        </Show>
        <Show when={view() === "learning"}>
          <LearningView models={models()} providerLabels={providerLabels()} />
        </Show>
        <Show when={view() === "settings"}>
          <SettingsView />
        </Show>
        <Show when={view() === "help"}>
          <HelpView onNavigate={(target) => setView(target)} />
        </Show>
      </main>

      <Show when={paletteOpen()}>
        <div class="modal-overlay" onClick={closePalette}>
          <div class="palette" onClick={(event) => event.stopPropagation()}>
            <input
              ref={(element) => setTimeout(() => element.focus(), 0)}
              value={paletteQuery()}
              placeholder="Раздел или действие… (↑↓, Enter)"
              onInput={(event) => {
                setPaletteQuery(event.currentTarget.value);
                setPaletteIndex(0);
              }}
              onKeyDown={(event) => {
                const list = filteredCommands();
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setPaletteIndex((index) => (index + 1) % Math.max(list.length, 1));
                } else if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setPaletteIndex(
                    (index) => (index - 1 + Math.max(list.length, 1)) % Math.max(list.length, 1),
                  );
                } else if (event.key === "Enter") {
                  event.preventDefault();
                  runActive();
                } else if (event.key === "Escape") {
                  closePalette();
                }
              }}
            />
            <div class="palette-list">
              <For each={filteredCommands()}>
                {(command, index) => (
                  <button
                    classList={{
                      "palette-item": true,
                      active:
                        paletteIndex() % Math.max(filteredCommands().length, 1) === index(),
                    }}
                    onClick={() => command.run()}
                  >
                    <span class="palette-icon">{command.icon}</span>
                    <span class="palette-label">{command.label}</span>
                    <span class="muted small">{command.hint}</span>
                  </button>
                )}
              </For>
              <Show when={filteredCommands().length === 0}>
                <p class="muted small pad">Ничего не найдено</p>
              </Show>
            </div>
            <div class="palette-foot muted small">
              Esc — закрыть · Ctrl+K — открыть
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
}
