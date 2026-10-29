import { For, Show, createSignal } from "solid-js";
import { api } from "../api";
import type { ModelInfo } from "../types";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  onModelsChanged: () => void;
}

export default function ModelsView(props: Props) {
  const [busy, setBusy] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);

  const local = () => props.models.filter((m) => m.kind === "local");
  const cloud = () => props.models.filter((m) => m.kind === "cloud");

  const load = async (model: ModelInfo) => {
    setBusy(model.id);
    setError(null);
    setNotice(null);
    try {
      const result = await api.modelsLoad(model.id.replace("local/", ""));
      setNotice(
        `${model.label}: загружена${result.load_seconds ? ` за ${result.load_seconds.toFixed(1)} с` : ""}`,
      );
      props.onModelsChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const unload = async (model: ModelInfo) => {
    setBusy(model.id);
    setError(null);
    try {
      await api.modelsUnload(model.id.replace("local/", ""));
      props.onModelsChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Модели</h1>
          <p class="muted small">
            Локальные модели OpenVINO IR. Одновременно держится не более одной тяжёлой модели
            (&gt; 8 ГБ) — менеджер сам выгружает лишнее.
          </p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

      <h2 class="section-title">Локальные</h2>
      <div class="card-grid">
        <For each={local()}>
          {(model) => (
            <section class="card model-card">
              <h3>{model.label}</h3>
              <p class="muted small">
                {model.device} · {model.size_gb?.toFixed(2)} ГБ
                {model.thinks ? " · умеет рассуждать" : ""}
              </p>
              <p classList={{ badge: true, on: model.loaded, off: !model.loaded }}>
                {model.loaded ? "загружена" : "не загружена"}
              </p>
              <div class="row">
                <button class="btn small" disabled={busy() !== null} onClick={() => load(model)}>
                  {busy() === model.id ? "Загрузка…" : "Загрузить"}
                </button>
                <button class="btn small ghost" disabled={busy() !== null} onClick={() => unload(model)}>
                  Выгрузить
                </button>
              </div>
            </section>
          )}
        </For>
      </div>

      <h2 class="section-title">Облачные</h2>
      <Show when={cloud().length === 0}>
        <p class="empty-inline">
          Нет облачных моделей. Добавьте провайдера на экране «Провайдеры» и обновите список моделей.
        </p>
      </Show>
      <div class="card-grid">
        <For each={cloud()}>
          {(model) => (
            <section class="card model-card">
              <h3>{model.label}</h3>
              <p class="muted small">
                {props.providerLabels[model.provider] ?? model.provider} · {model.id}
              </p>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
