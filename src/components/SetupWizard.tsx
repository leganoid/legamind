import { For, Show, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { EngineStats, ModelInfo } from "../types";

interface Props {
  models: ModelInfo[];
  onClose: () => void;
  onOpenModels: () => void;
}

export default function SetupWizard(props: Props) {
  const [step, setStep] = createSignal(0);
  const [stats, setStats] = createSignal<EngineStats | null>(null);
  const [checking, setChecking] = createSignal(false);
  const local = () => props.models.filter((model) => model.kind === "local");
  const cloud = () => props.models.filter((model) => model.kind === "cloud");

  const check = async () => {
    setChecking(true);
    try {
      setStats(await api.engineStats());
    } catch {
      setStats(null);
    } finally {
      setChecking(false);
    }
  };

  onMount(check);

  const finish = () => {
    localStorage.setItem("legamind.setup.v1.done", "1");
    props.onClose();
  };

  const openModels = () => {
    localStorage.setItem("legamind.setup.v1.done", "1");
    props.onClose();
    props.onOpenModels();
  };

  return (
    <div class="modal-overlay">
      <div class="tour-card setup-card">
        <div class="tour-progress">
          <For each={[0, 1, 2]}>
            {(index) => <span classList={{ "tour-dot": true, active: index === step() }} />}
          </For>
        </div>

        <Show when={step() === 0}>
          <h2>Добро пожаловать в Legamind</h2>
          <p>
            Это локальный помощник: модели, RAG и изображения работают на вашем компьютере.
            Облачные провайдеры подключаются отдельно.
          </p>
          <ul class="setup-list">
            <li>Данные и история остаются на диске.</li>
            <li>Телеметрии нет.</li>
            <li>Модели лежат в <span class="mono">C:\Models</span> или скачиваются в «Моделях».</li>
          </ul>
        </Show>

        <Show when={step() === 1}>
          <h2>Проверка системы</h2>
          <div class="setup-check">
            <span>Локальных моделей</span>
            <strong>{local().length}</strong>
          </div>
          <div class="setup-check">
            <span>Облачных моделей</span>
            <strong>{cloud().length}</strong>
          </div>
          <div class="setup-check">
            <span>Движок</span>
            <strong>{checking() ? "проверяю…" : stats() ? "запущен" : "не запущен"}</strong>
          </div>
          <Show when={stats()}>
            <p class="muted small">
              Загружено моделей: {stats()!.loaded.length}
              {stats()!.loaded.length > 0 ? ` (${stats()!.loaded.join(", ")})` : ""}
            </p>
          </Show>
          <button class="btn ghost" onClick={() => void check()} disabled={checking()}>
            Проверить снова
          </button>
        </Show>

        <Show when={step() === 2}>
          <h2>Модель для работы</h2>
          <Show
            when={local().length > 0}
            fallback={
              <p>
                Локальных моделей пока нет. Откройте «Модели» и скачайте рекомендованную
                Qwen3 4B или добавьте свою IR-папку.
              </p>
            }
          >
            <p>Найдены локальные модели. Для чата достаточно одной:</p>
            <ul class="setup-list">
              <For each={local().slice(0, 5)}>
                {(model) => (
                  <li>
                    <strong>{model.label}</strong>
                    <span class="muted small">
                      {" · "}
                      {model.device ?? "CPU"}
                      {model.size_gb ? ` · ${model.size_gb.toFixed(2)} ГБ` : ""}
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <button class="btn primary" onClick={openModels}>
            Открыть модели
          </button>
        </Show>

        <div class="row space-between setup-actions">
          <button class="btn ghost" onClick={finish}>
            Пропустить
          </button>
          <div class="row">
            <Show when={step() > 0}>
              <button class="btn ghost" onClick={() => setStep((value) => value - 1)}>
                Назад
              </button>
            </Show>
            <Show when={step() < 2}>
              <button class="btn primary" onClick={() => setStep((value) => value + 1)}>
                Далее
              </button>
            </Show>
            <Show when={step() === 2}>
              <button class="btn primary" onClick={finish}>
                Начать
              </button>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
}
