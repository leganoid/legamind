import { For, Show, createSignal } from "solid-js";
import { api } from "../api";
import type { CheckResult, ProviderView } from "../types";

interface Props {
  providers: ProviderView[];
  onChanged: () => void;
}

interface Preset {
  key: string;
  label: string;
  kind: "openai" | "anthropic";
  base_url: string;
}

const PRESETS: Preset[] = [
  { key: "openai", label: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1" },
  { key: "deepseek", label: "DeepSeek", kind: "openai", base_url: "https://api.deepseek.com/v1" },
  { key: "openrouter", label: "OpenRouter", kind: "openai", base_url: "https://openrouter.ai/api/v1" },
  { key: "anthropic", label: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com" },
  { key: "mimo", label: "MiMo (Xiaomi)", kind: "openai", base_url: "https://api.xiaomimimo.com/v1" },
  { key: "local", label: "Локальный сервер", kind: "openai", base_url: "http://127.0.0.1:8080/v1" },
  { key: "custom", label: "Свой", kind: "openai", base_url: "" },
];

export default function Providers(props: Props) {
  const [showForm, setShowForm] = createSignal(false);
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [presetKey, setPresetKey] = createSignal("deepseek");
  const [kind, setKind] = createSignal<"openai" | "anthropic">("openai");
  const [label, setLabel] = createSignal("");
  const [baseUrl, setBaseUrl] = createSignal("");
  const [apiKey, setApiKey] = createSignal("");
  const [timeoutS, setTimeoutS] = createSignal(120);
  const [busy, setBusy] = createSignal(false);
  const [check, setCheck] = createSignal<CheckResult | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const resetForm = () => {
    setEditingId(null);
    setPresetKey("deepseek");
    setKind("openai");
    setLabel("");
    setBaseUrl("");
    setApiKey("");
    setTimeoutS(120);
    setCheck(null);
    setError(null);
  };

  const openNew = () => {
    resetForm();
    const preset = PRESETS.find((p) => p.key === "deepseek")!;
    setLabel(preset.label);
    setBaseUrl(preset.base_url);
    setShowForm(true);
  };

  const openEdit = (provider: ProviderView) => {
    resetForm();
    setEditingId(provider.id);
    setKind(provider.kind);
    setLabel(provider.label);
    setBaseUrl(provider.base_url);
    setTimeoutS(provider.timeout_s);
    setShowForm(true);
  };

  const applyPreset = (key: string) => {
    setPresetKey(key);
    const preset = PRESETS.find((p) => p.key === key);
    if (!preset) return;
    setKind(preset.kind);
    if (preset.key !== "custom") {
      setBaseUrl(preset.base_url);
      if (!editingId()) setLabel(preset.label);
    }
  };

  const input = () => ({
    id: editingId(),
    kind: kind(),
    label: label(),
    base_url: baseUrl(),
    api_key: apiKey(),
    timeout_s: timeoutS(),
  });

  const runCheck = async () => {
    setBusy(true);
    setError(null);
    try {
      setCheck(await api.providerCheck(input()));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.providerSave(input());
      setShowForm(false);
      resetForm();
      props.onChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const refreshModels = async (id: string) => {
    setBusy(true);
    try {
      const result = await api.providerRefresh(id);
      setCheck(result);
      if (result.ok) props.onChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (provider: ProviderView) => {
    await api.providerSetEnabled(provider.id, !provider.enabled);
    props.onChanged();
  };

  const remove = async (provider: ProviderView) => {
    if (!window.confirm(`Удалить провайдера «${provider.label}» и его ключ?`)) return;
    await api.providerDelete(provider.id);
    props.onChanged();
  };

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Провайдеры</h1>
          <p class="muted small">
            Ключи хранятся в Windows Credential Manager и никогда не попадают в БД, логи и сайдкар.
          </p>
        </div>
        <button class="btn primary" onClick={openNew}>
          Добавить провайдера
        </button>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>

      <Show when={showForm()}>
        <section class="card form">
          <h2>{editingId() ? "Изменить провайдера" : "Новый провайдер"}</h2>
          <div class="form-grid">
            <label>
              Пресет
              <select value={presetKey()} onChange={(e) => applyPreset(e.currentTarget.value)}>
                <For each={PRESETS}>{(preset) => <option value={preset.key}>{preset.label}</option>}</For>
              </select>
            </label>
            <label>
              Тип API
              <select value={kind()} onChange={(e) => setKind(e.currentTarget.value as "openai" | "anthropic")}>
                <option value="openai">OpenAI-совместимый</option>
                <option value="anthropic">Anthropic</option>
              </select>
            </label>
            <label>
              Название
              <input value={label()} onInput={(e) => setLabel(e.currentTarget.value)} placeholder="DeepSeek" />
            </label>
            <label>
              Base URL
              <input
                value={baseUrl()}
                onInput={(e) => setBaseUrl(e.currentTarget.value)}
                placeholder="https://api.deepseek.com/v1"
              />
            </label>
            <label>
              API-ключ
              <input
                type="password"
                value={apiKey()}
                onInput={(e) => setApiKey(e.currentTarget.value)}
                placeholder={editingId() ? "оставьте пустым, чтобы не менять" : "sk-…"}
              />
            </label>
            <label>
              Таймаут, с
              <input
                type="number"
                min={10}
                max={600}
                value={timeoutS()}
                onInput={(e) => setTimeoutS(Number(e.currentTarget.value) || 120)}
              />
            </label>
          </div>
          <div class="form-actions">
            <button class="btn" disabled={busy()} onClick={runCheck}>
              Проверить
            </button>
            <button class="btn primary" disabled={busy() || !label() || !baseUrl()} onClick={save}>
              Сохранить
            </button>
            <button
              class="btn ghost"
              onClick={() => {
                setShowForm(false);
                resetForm();
              }}
            >
              Отмена
            </button>
            <Show when={check()}>
              <span classList={{ "check-result": true, ok: check()!.ok, bad: !check()!.ok }}>
                {check()!.message}
              </span>
            </Show>
          </div>
        </section>
      </Show>

      <Show when={props.providers.length === 0 && !showForm()}>
        <p class="empty-inline">Провайдеры не добавлены — приложение работает только локально.</p>
      </Show>

      <div class="provider-list">
        <For each={props.providers}>
          {(provider) => (
            <section class="card provider">
              <div class="provider-main">
                <h3>
                  {provider.label}
                  <span classList={{ badge: true, on: provider.enabled, off: !provider.enabled }}>
                    {provider.enabled ? "включён" : "выключен"}
                  </span>
                </h3>
                <p class="muted small">{provider.base_url}</p>
                <p class="muted small">
                  {provider.kind} · моделей: {provider.models_count} · ключ:{" "}
                  {provider.has_key ? provider.key_mask ?? "сохранён" : "не задан"}
                </p>
              </div>
              <div class="provider-actions">
                <button class="btn small" onClick={() => refreshModels(provider.id)} disabled={busy()}>
                  Обновить модели
                </button>
                <button class="btn small" onClick={() => toggle(provider)}>
                  {provider.enabled ? "Отключить" : "Включить"}
                </button>
                <button class="btn small" onClick={() => openEdit(provider)}>
                  Изменить
                </button>
                <button class="btn small danger" onClick={() => remove(provider)}>
                  Удалить
                </button>
              </div>
            </section>
          )}
        </For>
      </div>
    </div>
  );
}
