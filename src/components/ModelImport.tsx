import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  HfPreview,
  ModelDownloadEvent,
  ModelInspection,
} from "../types";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

function formatBytes(value: number) {
  if (!value) return "—";
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(2)} ГБ`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)} МБ`;
  return `${(value / 1024).toFixed(0)} КБ`;
}

export default function ModelImport(props: Props) {
  const [tab, setTab] = createSignal<"folder" | "hf">("folder");
  const [inspection, setInspection] = createSignal<ModelInspection | null>(null);
  const [label, setLabel] = createSignal("");
  const [device, setDevice] = createSignal("CPU");
  const [tokens, setTokens] = createSignal(2048);
  const [busy, setBusy] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const [repo, setRepo] = createSignal("");
  const [preview, setPreview] = createSignal<HfPreview | null>(null);
  const [progress, setProgress] = createSignal<ModelDownloadEvent | null>(null);
  const [hfToken, setHfToken] = createSignal("");
  const [tokenSaved, setTokenSaved] = createSignal(false);

  let unlisten: UnlistenFn | undefined;
  onMount(() => {
    void listen<ModelDownloadEvent>("model:download", (event) => {
      setProgress(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    void api.hfTokenStatus().then(setTokenSaved).catch(() => undefined);
  });
  onCleanup(() => unlisten?.());

  const applyInspection = (info: ModelInspection, suggested?: string) => {
    setInspection(info);
    setLabel(suggested?.trim() || info.label);
    if (info.context_tokens && info.context_tokens >= 512) {
      setTokens(Math.min(4096, info.context_tokens));
    }
  };

  const chooseFolder = async () => {
    setError(null);
    try {
      const dir = await api.pickFolder();
      if (!dir) return;
      applyInspection(await api.inspectLocalModel(dir));
    } catch (err) {
      setError(String(err));
    }
  };

  const previewRepo = async () => {
    setError(null);
    setPreview(null);
    setBusy("preview");
    try {
      const result = await api.hfModelPreview(repo().trim());
      setPreview(result);
      setLabel(result.label);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const downloadRepo = async () => {
    setError(null);
    setBusy("download");
    try {
      const path = await api.hfModelDownload(repo().trim());
      applyInspection(await api.inspectLocalModel(path), preview()?.label);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const saveToken = async () => {
    setError(null);
    setBusy("token");
    try {
      await api.hfTokenSet(hfToken());
      setTokenSaved(!!hfToken().trim());
      setHfToken("");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const convertRepo = async () => {
    setError(null);
    setBusy("convert");
    try {
      const path = await api.hfConvertModel(repo().trim());
      applyInspection(await api.inspectLocalModel(path), preview()?.label);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const add = async () => {
    const info = inspection();
    if (!info) return;
    setError(null);
    setBusy("add");
    try {
      await api.localModelAdd({
        path: info.path,
        label: label().trim() || info.label,
        device: device(),
        max_tokens: tokens(),
      });
      await api.engineRestart();
      props.onImported();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const percent = () => {
    const value = progress();
    if (!value || !value.total) return 0;
    return Math.min(100, Math.round((value.downloaded / value.total) * 100));
  };

  return (
    <div class="modal-overlay" onClick={() => props.onClose()}>
      <div class="modal import-modal" onClick={(event) => event.stopPropagation()}>
        <header class="row space-between">
          <div>
            <h2>Мастер локальных моделей</h2>
            <p class="muted small">
              Папка OpenVINO IR или готовая модель с Hugging Face. Файлы остаются у вас.
            </p>
          </div>
          <button class="icon-btn" onClick={() => props.onClose()}>
            ✕
          </button>
        </header>

        <div class="row">
          <button
            classList={{ tab: true, active: tab() === "folder" }}
            onClick={() => setTab("folder")}
          >
            📁 Папка
          </button>
          <button
            classList={{ tab: true, active: tab() === "hf" }}
            onClick={() => setTab("hf")}
          >
            🌐 Hugging Face
          </button>
        </div>

        <Show when={error()}>
          <div class="banner error">{error()}</div>
        </Show>

        <Show when={tab() === "folder"}>
          <div class="row">
            <button class="btn" onClick={() => void chooseFolder()} disabled={busy() !== null}>
              Выбрать папку…
            </button>
            <span class="muted small">
              В папке должны быть openvino_model.xml и .bin
            </span>
          </div>
        </Show>

        <Show when={tab() === "hf"}>
          <label>
            Репозиторий Hugging Face
            <div class="row">
              <input
                value={repo()}
                placeholder="OpenVINO/Qwen2.5-1.5B-Instruct-int4-ov"
                onInput={(event) => setRepo(event.currentTarget.value)}
              />
              <button
                class="btn"
                onClick={() => void previewRepo()}
                disabled={!repo().trim() || busy() !== null}
              >
                {busy() === "preview" ? "Проверяю…" : "Проверить"}
              </button>
            </div>
          </label>
          <p class="muted small">
            Токен нужен для gated/private репозиториев. Хранится в Credential Manager.
            {tokenSaved() ? " Токен сохранён." : ""}
          </p>
          <div class="row">
            <input
              type="password"
              value={hfToken()}
              placeholder={tokenSaved() ? "Токен уже сохранён" : "hf_..."}
              onInput={(event) => setHfToken(event.currentTarget.value)}
            />
            <button
              class="btn ghost"
              onClick={() => void saveToken()}
              disabled={busy() !== null}
            >
              {busy() === "token" ? "Сохраняю…" : "Сохранить токен"}
            </button>
          </div>
          <p class="muted small">
            Если в репозитории нет IR, можно конвертировать PyTorch → OpenVINO через
            Optimum (нужны <code>optimum[openvino]</code>, <code>transformers</code>,{" "}
            <code>torch</code>).
          </p>
          <button
            class="btn ghost"
            onClick={() => void convertRepo()}
            disabled={!repo().trim() || busy() !== null}
          >
            {busy() === "convert" ? "Конвертирую… (долго)" : "Конвертировать в IR"}
          </button>
          <Show when={preview()}>
            <div class="import-preview">
              <p>
                <strong>{preview()!.repo}</strong> · файлов: {preview()!.files.length} ·{" "}
                {formatBytes(preview()!.total_bytes)}
              </p>
              <p class="muted small">
                IR: {preview()!.ir_ready ? "готов" : "нет"} · токенизатор:{" "}
                {preview()!.tokenizer_ready ? "есть" : "нет"}
              </p>
              <button
                class="btn primary"
                onClick={() => void downloadRepo()}
                disabled={busy() !== null}
              >
                {busy() === "download" ? "Скачиваю…" : "Скачать и проверить"}
              </button>
            </div>
          </Show>
          <Show when={progress()}>
            <div class="import-progress">
              <div class="import-progress-bar">
                <div style={{ width: `${percent()}%` }} />
              </div>
              <p class="muted small">
                {percent()}% · {formatBytes(progress()!.downloaded)} из{" "}
                {formatBytes(progress()!.total)} · файл {progress()!.file_index}/
                {progress()!.file_count}
                {progress()!.file ? ` (${progress()!.file})` : ""}
              </p>
            </div>
          </Show>
        </Show>

        <Show when={inspection()}>
          <div class="import-preview">
            <p>
              <strong>{inspection()!.label}</strong> · {inspection()!.size_gb} ГБ
            </p>
            <p class="muted small">
              XML: {inspection()!.has_xml ? "да" : "нет"} · BIN:{" "}
              {inspection()!.has_bin ? "да" : "нет"} · токенизатор:{" "}
              {inspection()!.has_tokenizer ? "есть" : "нет"}
              {inspection()!.context_tokens
                ? ` · контекст: ${inspection()!.context_tokens}`
                : ""}
            </p>
            <div class="row">
              <label>
                Название
                <input
                  value={label()}
                  onInput={(event) => setLabel(event.currentTarget.value)}
                />
              </label>
              <label>
                Устройство
                <select value={device()} onChange={(e) => setDevice(e.currentTarget.value)}>
                  <option value="CPU">CPU</option>
                  <option value="GPU">GPU</option>
                  <option value="NPU">NPU</option>
                </select>
              </label>
              <label>
                Лимит ответа, токенов
                <input
                  type="number"
                  min="512"
                  max="32768"
                  value={tokens()}
                  onInput={(event) => setTokens(Number(event.currentTarget.value) || 2048)}
                />
              </label>
            </div>
            <button class="btn primary" onClick={() => void add()} disabled={busy() !== null}>
              {busy() === "add" ? "Добавляю…" : "Добавить и перезапустить движок"}
            </button>
          </div>
        </Show>

        <Show when={!inspection() && tab() === "folder"}>
          <p class="muted small">Выберите папку — покажу размер, контекст и состав.</p>
        </Show>
        <Show when={!inspection() && tab() === "hf" && !preview()}>
          <p class="muted small">Укажите репозиторий и нажмите «Проверить».</p>
        </Show>
      </div>
    </div>
  );
}
