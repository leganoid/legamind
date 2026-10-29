import { Show, createMemo, createSignal, onMount } from "solid-js";
import { api } from "./api";
import type { ModelInfo, ProviderView } from "./types";
import ChatView from "./components/ChatView";
import DebateView from "./components/DebateView";
import ModelsView from "./components/ModelsView";
import Providers from "./components/Providers";
import MemoryPanel from "./components/MemoryPanel";
import SettingsView from "./components/SettingsView";
import SkillsView from "./components/SkillsView";

type View = "chat" | "debate" | "models" | "providers" | "memory" | "skills" | "settings";

const NAV: { key: View; label: string; icon: string }[] = [
  { key: "chat", label: "Чат", icon: "💬" },
  { key: "debate", label: "Дебаты", icon: "⚖" },
  { key: "models", label: "Модели", icon: "🧩" },
  { key: "providers", label: "Провайдеры", icon: "🔑" },
  { key: "memory", label: "Память", icon: "🧠" },
  { key: "skills", label: "Скиллы", icon: "🧰" },
  { key: "settings", label: "Настройки", icon: "⚙" },
];

export default function App() {
  const [view, setView] = createSignal<View>("chat");
  const [models, setModels] = createSignal<ModelInfo[]>([]);
  const [providers, setProviders] = createSignal<ProviderView[]>([]);
  const [error, setError] = createSignal<string | null>(null);

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

  onMount(async () => {
    await refreshProviders();
    const timer = window.setInterval(refreshModels, 15000);
    window.addEventListener("keydown", (event) => {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setView("chat");
        window.dispatchEvent(new Event("legamind:new-session"));
      }
    });
    window.addEventListener("beforeunload", () => window.clearInterval(timer));
  });

  return (
    <div class="app">
      <nav class="nav">
        <div class="brand">
          <span class="brand-mark">H</span>
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
        <Show when={view() === "settings"}>
          <SettingsView />
        </Show>
      </main>
    </div>
  );
}
