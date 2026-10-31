import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { api } from "../api";
import type { EngineStats, LocalModelRow, ModelInfo } from "../types";
import ConfirmDialog from "./ConfirmDialog";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  onModelsChanged: () => void;
}

export default function ModelsView(props: Props) {
  const [busy, setBusy] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [custom, setCustom] = createSignal<LocalModelRow[]>([]);
  const [modelPath, setModelPath] = createSignal("");
  const [modelLabel, setModelLabel] = createSignal("");
  const [modelDevice, setModelDevice] = createSignal("CPU");
  const [modelTokens, setModelTokens] = createSignal(2048);
  const [modelThinks, setModelThinks] = createSignal(false);
  const [removeTarget, setRemoveTarget] = createSignal<LocalModelRow | null>(null);
  const [stats, setStats] = createSignal<EngineStats | null>(null);

  const loadStats = async () => {
    try {
      setStats(await api.engineStats());
    } catch {
      setStats(null);
    }
  };

  onMount(() => {
    void loadStats();
    const timer = window.setInterval(() => void loadStats(), 5000);
    onCleanup(() => window.clearInterval(timer));
  });

  const loadCustom = async () => {
    try {
      setCustom(await api.localModelsList());
    } catch (err) {
      setError(String(err));
    }
  };

  onMount(loadCustom);

  const chooseModelFolder = async () => {
    try {
      const dir = await api.pickFolder();
      if (!dir) return;
      setModelPath(dir);
      if (!modelLabel().trim()) {
        const parts = dir.split(/[\\/]/).filter(Boolean);
        setModelLabel(parts[parts.length - 1] ?? "Локальная модель");
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const addModel = async () => {
    setBusy("add-model");
    setError(null);
    setNotice("Добавляю модель и перезапускаю движок…");
    try {
      await api.localModelAdd({
        path: modelPath(),
        label: modelLabel(),
        device: modelDevice(),
        max_tokens: modelTokens(),
        thinks: modelThinks(),
      });
      await api.engineRestart();
      await loadCustom();
      props.onModelsChanged();
      setNotice("Модель добавлена в конфиг движка");
      setModelPath("");
      setModelLabel("");
      setModelThinks(false);
    } catch (err) {
      setNotice(null);
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const removeModel = async () => {
    const target = removeTarget();
    setRemoveTarget(null);
    if (!target) return;
    setBusy("remove-model");
    setError(null);
    try {
      await api.localModelRemove(target.id);
      await api.engineRestart();
      await loadCustom();
      props.onModelsChanged();
      setNotice(`Модель «${target.label}» убрана из конфига`);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

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

      <Show when={stats()}>
        <div class="card">
          <div class="row space-between">
            <strong>Движок</strong>
            <span class="muted small">
              {stats()!.rss_mb ? `${stats()!.rss_mb!.toFixed(1)} МБ RSS` : "RSS недоступен"} ·
              моделей в реестре: {stats()!.models}
            </span>
          </div>
          <p class="muted small">
            {stats()!.loaded.length > 0
              ? `Загружено: ${stats()!.loaded.join(", ")}`
              : "Сейчас ни одна модель не загружена"}
          </p>
        </div>
      </Show>

      <section class="card form">
        <h2>Добавить локальную модель</h2>
        <p class="muted small">
          Укажите папку с OpenVINO IR (файлы <code>.xml</code> и <code>.bin</code>). Модель
          сохранится в конфиге движка; после добавления он перезапустится. Файлы модели не
          копируются и не удаляются.
        </p>
        <div class="form-grid">
          <label>
            Папка модели
            <div class="row">
              <input value={modelPath()} readOnly placeholder="C:\Models\my-model-int4-ov" />
              <button class="btn small" onClick={() => void chooseModelFolder()}>
                Выбрать…
              </button>
            </div>
          </label>
          <label>
            Название
            <input
              value={modelLabel()}
              onInput={(event) => setModelLabel(event.currentTarget.value)}
              placeholder="Например, Qwen3-8B"
            />
          </label>
          <label>
            Устройство
            <select
              value={modelDevice()}
              onChange={(event) => setModelDevice(event.currentTarget.value)}
            >
              <option value="CPU">CPU</option>
              <option value="GPU">GPU</option>
              <option value="NPU">NPU</option>
            </select>
          </label>
          <label>
            Макс. токенов
            <input
              type="number"
              min="512"
              max="32768"
              value={modelTokens()}
              onInput={(event) => setModelTokens(Number(event.currentTarget.value) || 2048)}
            />
          </label>
        </div>
        <label class="inline">
          <input
            type="checkbox"
            checked={modelThinks()}
            onChange={(event) => setModelThinks(event.currentTarget.checked)}
          />
          Модель умеет рассуждать (thinking)
        </label>
        <div class="row">
          <button
            class="btn primary"
            disabled={!modelPath().trim() || !modelLabel().trim() || busy() !== null}
            onClick={() => void addModel()}
          >
            {busy() === "add-model" ? "Добавляю…" : "Добавить и перезапустить"}
          </button>
        </div>
      </section>

      <Show when={custom().length > 0}>
        <h2 class="section-title">Добавленные вручную</h2>
        <div class="card-grid">
          <For each={custom()}>
            {(model) => (
              <section class="card model-card">
                <h3>{model.label}</h3>
                <p class="muted small">
                  {model.device} · {model.size_gb.toFixed(2)} ГБ · {model.max_tokens} ток.
                </p>
                <p class="muted small mono">{model.path}</p>
                <button
                  class="btn small ghost"
                  disabled={busy() !== null}
                  onClick={() => setRemoveTarget(model)}
                >
                  Убрать из списка
                </button>
              </section>
            )}
          </For>
        </div>
      </Show>

      <h2 class="section-title">Локальные</h2>
      <div class="card-grid">
        <For each={local()}>
          {(model) => (
            <section class="card model-card">
              <h3>{model.label}</h3>
              <p class="muted small">
                {model.device} · {model.size_gb?.toFixed(2)} ГБ
                {model.context ? ` · контекст ${model.context} ток.` : ""}
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

      <ConfirmDialog
        open={removeTarget() !== null}
        title="Убрать модель из списка?"
        message={`«${removeTarget()?.label ?? ""}» исчезнет из конфига движка. Файлы на диске останутся на месте.`}
        confirmLabel="Убрать"
        danger
        onConfirm={() => void removeModel()}
        onCancel={() => setRemoveTarget(null)}
      />
    </div>
  );
}
