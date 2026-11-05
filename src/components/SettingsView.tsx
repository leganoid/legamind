import { For, Show, createSignal, onMount } from "solid-js";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "../api";
import type { AppInfo, BackupInfo, LogTail, ModelInfo, ModelUsage, SelfCheckItem, WebConfigView, WebTestResult } from "../types";
import ConfirmDialog from "./ConfirmDialog";

export default function SettingsView() {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const [web, setWeb] = createSignal<WebConfigView | null>(null);
  const [provider, setProvider] = createSignal("auto");
  const [quality, setQuality] = createSignal("strict");
  const [searxngUrl, setSearxngUrl] = createSignal("");
  const [tavilyKey, setTavilyKey] = createSignal("");
  const [braveKey, setBraveKey] = createSignal("");
  const [googleKey, setGoogleKey] = createSignal("");
  const [googleCx, setGoogleCx] = createSignal("");
  const [webTest, setWebTest] = createSignal<WebTestResult | null>(null);
  const [testing, setTesting] = createSignal(false);
  const [resetOpen, setResetOpen] = createSignal(false);
  const [restoreOpen, setRestoreOpen] = createSignal(false);
  const [dbBusy, setDbBusy] = createSignal(false);
  const [encryptBackup, setEncryptBackup] = createSignal(false);
  const [backupInfo, setBackupInfo] = createSignal<BackupInfo | null>(null);
  const [logName, setLogName] = createSignal("app.log");
  const [logLines, setLogLines] = createSignal(400);
  const [logTail, setLogTail] = createSignal<LogTail | null>(null);
  const [logBusy, setLogBusy] = createSignal(false);
  const [checks, setChecks] = createSignal<SelfCheckItem[]>([]);
  const [checkBusy, setCheckBusy] = createSignal(false);
  const [embeddingModels, setEmbeddingModels] = createSignal<ModelInfo[]>([]);
  const [embeddingModel, setEmbeddingModel] = createSignal("");
  const [embeddingBusy, setEmbeddingBusy] = createSignal(false);
  const [rerankerModel, setRerankerModel] = createSignal("");
  const [rerankerBusy, setRerankerBusy] = createSignal(false);
  const [ragExtensions, setRagExtensions] = createSignal("");
  const [ragExcludes, setRagExcludes] = createSignal("");
  const [ragFileKb, setRagFileKb] = createSignal(256);
  const [ragMaxFiles, setRagMaxFiles] = createSignal(400);
  const [ragMaxChunks, setRagMaxChunks] = createSignal(2000);
  const [ragBusy, setRagBusy] = createSignal(false);

  const saveRagConfig = async () => {
    setRagBusy(true);
    setError(null);
    try {
      const saved = await api.ragConfigSet({
        extensions: ragExtensions().split(",").map((value) => value.trim()).filter(Boolean),
        excludes: ragExcludes().split(",").map((value) => value.trim()).filter(Boolean),
        max_file_kb: ragFileKb(),
        max_files: ragMaxFiles(),
        max_chunks: ragMaxChunks(),
      });
      setRagExtensions(saved.extensions.join(", "));
      setRagExcludes(saved.excludes.join(", "));
      setRagFileKb(saved.max_file_kb);
      setRagMaxFiles(saved.max_files);
      setRagMaxChunks(saved.max_chunks);
      setNotice("Настройки RAG сохранены. Переиндексируйте проект.");
    } catch (err) {
      setError(String(err));
    } finally {
      setRagBusy(false);
    }
  };

  const saveReranker = async () => {
    setRerankerBusy(true);
    setError(null);
    try {
      await api.ragRerankerSet(rerankerModel() || null);
      setNotice(
        rerankerModel()
          ? "Реранкер сохранён: кандидаты RAG будут переупорядочиваться."
          : "Реранкер отключён.",
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setRerankerBusy(false);
    }
  };
  const [updateBusy, setUpdateBusy] = createSignal(false);
  const [cloudEnabled, setCloudEnabled] = createSignal(false);
  const [cloudUrl, setCloudUrl] = createSignal("");
  const [cloudUser, setCloudUser] = createSignal("");
  const [cloudPassword, setCloudPassword] = createSignal("");
  const [cloudPassphrase, setCloudPassphrase] = createSignal("");
  const [cloudHasPassword, setCloudHasPassword] = createSignal(false);
  const [cloudBusy, setCloudBusy] = createSignal(false);

  const saveCloud = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      const saved = await api.cloudSet(
        cloudEnabled(),
        cloudUrl(),
        cloudUser(),
        cloudPassword(),
      );
      setCloudHasPassword(saved.has_password);
      setCloudPassword("");
      setNotice("Настройки WebDAV сохранены");
    } catch (err) {
      setError(String(err));
    } finally {
      setCloudBusy(false);
    }
  };

  const uploadCloud = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      setNotice(await api.cloudUpload(cloudPassphrase()));
    } catch (err) {
      setError(String(err));
    } finally {
      setCloudBusy(false);
    }
  };

  const restoreCloud = async () => {
    if (!window.confirm("Восстановить базу из WebDAV? Текущая база будет заменена.")) return;
    setCloudBusy(true);
    setError(null);
    try {
      setNotice(await api.cloudRestore(cloudPassphrase()));
    } catch (err) {
      setError(String(err));
    } finally {
      setCloudBusy(false);
    }
  };

  const [usage, setUsage] = createSignal<ModelUsage[]>([]);
  const [usageBusy, setUsageBusy] = createSignal(false);
  const [prices, setPrices] = createSignal<Record<string, number>>({});
  const [pricesBusy, setPricesBusy] = createSignal(false);

  const loadUsage = async () => {
    setUsageBusy(true);
    try {
      setUsage(await api.usageStats());
      setPrices(await api.usagePricesGet());
    } catch (err) {
      setError(String(err));
    } finally {
      setUsageBusy(false);
    }
  };

  const savePrices = async () => {
    setPricesBusy(true);
    try {
      await api.usagePricesSet(prices());
      setNotice("Цены сохранены: оценка считается по выходным токенам.");
    } catch (err) {
      setError(String(err));
    } finally {
      setPricesBusy(false);
    }
  };

  const setPrice = (modelId: string, value: string) => {
    const parsed = Number(value);
    setPrices((prev) => ({
      ...prev,
      [modelId]: Number.isFinite(parsed) && parsed > 0 ? parsed : 0,
    }));
  };

  const totalTokens = () => usage().reduce((sum, row) => sum + row.tokens, 0);
  const totalInput = () => usage().reduce((sum, row) => sum + row.input_tokens, 0);
  const totalMessages = () => usage().reduce((sum, row) => sum + row.messages, 0);
  const costFor = (row: ModelUsage) =>
    (row.tokens / 1_000_000) * (prices()[row.model_id] ?? 0);
  const totalCost = () => usage().reduce((sum, row) => sum + costFor(row), 0);

  const checkUpdates = async () => {
    setUpdateBusy(true);
    setError(null);
    setNotice(null);
    try {
      const update = await check();
      if (!update) {
        setNotice("Обновлений нет — установлена последняя версия");
        return;
      }
      setNotice(`Доступно обновление ${update.version}. Скачиваю и устанавливаю…`);
      await update.downloadAndInstall();
      setNotice("Обновление установлено, перезапускаю…");
      await relaunch();
    } catch (err) {
      setError(String(err));
    } finally {
      setUpdateBusy(false);
    }
  };

  const saveEmbedding = async () => {
    setEmbeddingBusy(true);
    setError(null);
    try {
      await api.ragEmbeddingSet(embeddingModel() || null);
      setNotice(
        embeddingModel()
          ? "Модель эмбеддингов сохранена. Векторизуйте проект в меню «Проект»."
          : "Векторный поиск отключён, RAG работает на FTS5.",
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setEmbeddingBusy(false);
    }
  };

  const runSelfCheck = async () => {
    setCheckBusy(true);
    setError(null);
    try {
      setChecks(await api.selfCheck());
    } catch (err) {
      setError(String(err));
    } finally {
      setCheckBusy(false);
    }
  };
  const [theme, setTheme] = createSignal(
    localStorage.getItem("legamind.theme") === "light" ? "light" : "dark",
  );

  const applyTheme = (value: "dark" | "light") => {
    setTheme(value);
    localStorage.setItem("legamind.theme", value);
    document.documentElement.dataset.theme = value;
  };
  const [health, setHealth] = createSignal<
    { engine: string; search: string; ok: boolean } | null
  >(null);
  const [checking, setChecking] = createSignal(false);
  const savedTab = localStorage.getItem("legamind.settings.tab");
  const [tab, setTab] = createSignal<"internet" | "app" | "privacy">(
    savedTab === "app" || savedTab === "privacy" ? savedTab : "internet",
  );

  const chooseTab = (value: "internet" | "app" | "privacy") => {
    setTab(value);
    localStorage.setItem("legamind.settings.tab", value);
  };

  const load = async () => setInfo(await api.appInfo());

  const loadWeb = async () => {
    const config = await api.webConfigGet();
    setWeb(config);
    setProvider(config.provider);
    setQuality(config.quality);
    setSearxngUrl(config.searxng_url);
    setGoogleCx(config.google_cx);
  };

  const loadLog = async () => {
    setLogBusy(true);
    setError(null);
    try {
      setLogTail(await api.logsTail(logName(), logLines()));
    } catch (err) {
      setError(String(err));
    } finally {
      setLogBusy(false);
    }
  };

  const loadBackupInfo = async () => {    try {
      setBackupInfo(await api.dbBackupInfo());
    } catch {
      setBackupInfo(null);
    }
  };

  const saveWeb = async (patch: { enabled?: boolean }) => {
    setError(null);
    try {
      const config = await api.webConfigSet({
        enabled: patch.enabled,
        provider: provider(),
        quality: quality(),
        searxng_url: searxngUrl(),
        google_cx: googleCx(),
        tavily_key: tavilyKey().trim() || undefined,
        brave_key: braveKey().trim() || undefined,
        google_key: googleKey().trim() || undefined,
      });
      setWeb(config);
      setTavilyKey("");
      setBraveKey("");
      setGoogleKey("");
      setNotice("Настройки интернета сохранены");
    } catch (err) {
      setError(String(err));
    }
  };

  const testSearch = async () => {
    setTesting(true);
    setError(null);
    setWebTest(null);
    try {
      setWebTest(await api.webTest());
    } catch (err) {
      setError(String(err));
    } finally {
      setTesting(false);
    }
  };

  const checkAll = async () => {
    setChecking(true);
    setError(null);
    setHealth(null);
    try {
      const appInfo = await api.appInfo();
      let search = "не проверялся";
      let ok = true;
      try {
        const test = await api.webTest();
        if (test.error) {
          search = `ошибка: ${test.error}`;
          ok = false;
        } else {
          search = `${test.provider}: ${test.results.length} результатов`;
        }
      } catch (err) {
        search = String(err);
        ok = false;
      }
      setHealth({
        engine: appInfo.engine_status ?? "неизвестно",
        search,
        ok,
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setChecking(false);
    }
  };

  const resetWeb = async () => {
    setResetOpen(false);
    setProvider("auto");
    setSearxngUrl("");
    setGoogleCx("");
    setTavilyKey("");
    setBraveKey("");
    setGoogleKey("");
    try {
      const config = await api.webConfigSet({
        enabled: true,
        provider: "auto",
        searxng_url: "",
        google_cx: "",
      });
      setWeb(config);
      setNotice("Интернет-настройки сброшены: провайдер «Авто», поля очищены");
    } catch (err) {
      setError(String(err));
    }
  };

  const exportAllData = async () => {
    setDbBusy(true);
    setError(null);
    try {
      const path = await api.exportAll();
      setNotice(`Все чаты экспортированы: ${path}`);
    } catch (err) {
      setError(String(err));
    } finally {
      setDbBusy(false);
    }
  };

  const backupDb = async () => {    setDbBusy(true);
    setError(null);
    try {
      const path = await api.dbBackup(encryptBackup());
      setNotice(`Копия базы сохранена: ${path}`);
      await loadBackupInfo();
    } catch (err) {
      const message = String(err);
      if (!message.toLowerCase().includes("отмен")) setError(message);
    } finally {
      setDbBusy(false);
    }
  };

  const restoreDb = async () => {
    setRestoreOpen(false);
    setDbBusy(true);
    setError(null);
    try {
      const message = await api.dbRestore();
      setNotice(`${message}. Приложение перезапускается…`);
    } catch (err) {
      const message = String(err);
      if (!message.toLowerCase().includes("отмен")) setError(message);
      setDbBusy(false);
    }
  };

  const restart = async () => {    setError(null);
    setNotice("Перезапуск движка…");
    try {
      const port = await api.engineRestart();
      setNotice(`Движок запущен на порту ${port}`);
      await load();
    } catch (err) {
      setNotice(null);
      setError(String(err));
    }
  };

  onMount(async () => {
    await load();
    await loadWeb();
    await loadBackupInfo();
    setEmbeddingModel((await api.ragEmbeddingGet()) ?? "");
    setRerankerModel((await api.ragRerankerGet()) ?? "");
    const cloud = await api.cloudGet();
    setCloudEnabled(cloud.enabled);
    setCloudUrl(cloud.url);
    setCloudUser(cloud.user);
    setCloudHasPassword(cloud.has_password);
    const rag = await api.ragConfigGet();
    setRagExtensions(rag.extensions.join(", "));
    setRagExcludes(rag.excludes.join(", "));
    setRagFileKb(rag.max_file_kb);
    setRagMaxFiles(rag.max_files);
    setRagMaxChunks(rag.max_chunks);
    void loadUsage();
    const all = await api.modelsList();
    setEmbeddingModels(
      all.filter((model) => model.role === "embedding" || model.kind === "local"),
    );
  });

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Настройки</h1>
          <p class="muted small">
            Интернет для моделей, состояние приложения и приватность — по разделам.
          </p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

      <div class="tabs">
        <button
          classList={{ tab: true, active: tab() === "internet" }}
          onClick={() => chooseTab("internet")}
        >
          🌐 Интернет
        </button>
        <button classList={{ tab: true, active: tab() === "app" }} onClick={() => chooseTab("app")}>
          ⚙ Приложение
        </button>
        <button
          classList={{ tab: true, active: tab() === "privacy" }}
          onClick={() => chooseTab("privacy")}
        >
          🔒 Приватность
        </button>
      </div>

      <Show when={tab() === "internet"}>
      <section class="card form">
        <h2>Интернет для моделей</h2>
        <p class="muted small">
          Если включено, перед ответом Legamind ищет запрос в интернете: Google Custom Search
          (ключ + CX), Tavily, Brave или свой SearxNG; без ключей — Wikipedia, Stack Exchange,
          GitHub, arXiv и Yahoo как резерв. Источники помечаются уровнем доверия
          (документация → научные работы → сообщества → блоги), а в строгом режиме блоги
          отбрасываются. Найденное вставляется в контекст, ссылки сохраняются под ответом.
        </p>

        <Show when={web()}>
          <div class="row">
            <label class="inline">
              <input
                type="checkbox"
                checked={web()!.enabled}
                onChange={(e) => saveWeb({ enabled: e.currentTarget.checked })}
              />
              Включён
            </label>
          </div>

          <div class="form-grid">
            <label>
              Провайдер поиска
              <select value={provider()} onChange={(e) => setProvider(e.currentTarget.value)}>
                <option value="auto">Авто: качество → ключи → arXiv → Wikipedia → SO → GitHub</option>
                <option value="wikipedia">Wikipedia (без ключа)</option>
                <option value="stackexchange">Stack Exchange (без ключа)</option>
                <option value="github">GitHub (без ключа)</option>
                <option value="arxiv">arXiv — научные статьи (без ключа)</option>
                <option value="searxng">SearxNG (свой инстанс)</option>
                <option value="tavily">Tavily (API-ключ)</option>
                <option value="brave">Brave Search (API-ключ)</option>
                <option value="google">Google Custom Search (ключ + CX)</option>
                <option value="yandex">Yandex (best effort, часто капча)</option>
                <option value="yahoo">Yahoo (последний резерв)</option>
                <option value="bing">Bing (best effort)</option>
                <option value="ddg">DuckDuckGo (best effort)</option>
              </select>
            </label>
            <label>
              Качество источников
              <select value={quality()} onChange={(e) => setQuality(e.currentTarget.value)}>
                <option value="strict">
                  Строгое: документация, научные работы, сообщества
                </option>
                <option value="balanced">Сбалансированное: всё, по убыванию доверия</option>
                <option value="open">Открытое: как нашлось</option>
              </select>
            </label>
            <Show when={provider() === "searxng"}>
              <label>
                Адрес SearxNG
                <input
                  value={searxngUrl()}
                  onInput={(e) => setSearxngUrl(e.currentTarget.value)}
                  placeholder="https://searx.example.com"
                />
              </label>
            </Show>
            <Show when={provider() === "tavily"}>
              <label>
                Ключ Tavily {web()!.has_tavily ? "(сохранён)" : ""}
                <input
                  type="password"
                  value={tavilyKey()}
                  onInput={(e) => setTavilyKey(e.currentTarget.value)}
                  placeholder="tvly-…"
                />
              </label>
            </Show>
            <Show when={provider() === "brave"}>
              <label>
                Ключ Brave {web()!.has_brave ? "(сохранён)" : ""}
                <input
                  type="password"
                  value={braveKey()}
                  onInput={(e) => setBraveKey(e.currentTarget.value)}
                  placeholder="BSA…"
                />
              </label>
            </Show>
            <Show when={provider() === "google" || provider() === "auto"}>
              <label>
                Google API-ключ {web()!.has_google ? "(сохранён)" : ""}
                <input
                  type="password"
                  value={googleKey()}
                  onInput={(e) => setGoogleKey(e.currentTarget.value)}
                  placeholder="AIza…"
                />
              </label>
              <label>
                Google CX (Programmable Search Engine ID)
                <input
                  value={googleCx()}
                  onInput={(e) => setGoogleCx(e.currentTarget.value)}
                  placeholder="например 0123456789:abcdefghijk"
                />
              </label>
            </Show>
          </div>

          <div class="row">
            <button class="btn primary" onClick={() => saveWeb({})}>
              Сохранить
            </button>
            <button class="btn" onClick={testSearch} disabled={testing()}>
              {testing() ? "Проверяю…" : "Проверить поиск"}
            </button>
            <button class="btn ghost" onClick={() => setResetOpen(true)}>
              Сбросить интернет-настройки
            </button>
          </div>

          <Show when={webTest()}>
            <div class="web-test">
              <div class="muted small">Провайдер: {webTest()!.provider}</div>
              <Show when={webTest()!.error}>
                <div class="banner error">{webTest()!.error}</div>
              </Show>
              <ol>
                <For each={webTest()!.results}>
                  {(result) => (
                    <li>
                      <a href={result.url} onClick={(e) => e.preventDefault()}>
                        {result.title}
                      </a>
                      <div class="muted small">{result.url}</div>
                      <div class="muted small">{result.snippet}</div>
                    </li>
                  )}
                </For>
              </ol>
            </div>
          </Show>
        </Show>
      </section>
      </Show>

      <Show when={tab() === "app"}>
      <section class="card">
        <h2>Обновления</h2>
        <p class="muted small">
          Текущая версия: {info()?.version ?? "…"}. Обновления скачиваются с GitHub Releases.
        </p>
        <button class="btn" onClick={() => void checkUpdates()} disabled={updateBusy()}>
          {updateBusy() ? "Проверяю…" : "🔄 Проверить обновления"}
        </button>
      </section>
      <section class="card">
        <h2>Расход токенов</h2>
        <p class="muted small">
          Всего: {totalMessages()} ответов, вход: {totalInput().toLocaleString("ru-RU")},{" "}
          выход: {totalTokens().toLocaleString("ru-RU")}. Оценка:{" "}
          <strong>${totalCost().toFixed(4)}</strong> — по цене за 1M выходных токенов.
        </p>
        <Show when={usage().length > 0} fallback={<p class="muted small">Пока нет данных.</p>}>
          <table class="usage-table">
            <thead>
              <tr>
                <th>Модель</th>
                <th>Ответов</th>
                <th>Вход</th>
                <th>Выход</th>
                <th>Цена за 1M, $</th>
                <th>Оценка</th>
              </tr>
            </thead>
            <tbody>
              <For each={usage()}>
                {(row) => (
                  <tr>
                    <td class="mono">{row.model_id}</td>
                    <td>{row.messages}</td>
                    <td>{row.input_tokens.toLocaleString("ru-RU")}</td>
                    <td>{row.tokens.toLocaleString("ru-RU")}</td>
                    <td>
                      <input
                        class="price-input"
                        type="number"
                        min="0"
                        step="0.01"
                        placeholder="0"
                        value={prices()[row.model_id] || ""}
                        onInput={(event) =>
                          setPrice(row.model_id, event.currentTarget.value)
                        }
                      />
                    </td>
                    <td>${costFor(row).toFixed(4)}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
        <div class="row">
          <button class="btn ghost" onClick={() => void loadUsage()} disabled={usageBusy()}>
            {usageBusy() ? "Обновляю…" : "Обновить"}
          </button>
          <button class="btn" onClick={() => void savePrices()} disabled={pricesBusy()}>
            {pricesBusy() ? "Сохраняю…" : "Сохранить цены"}
          </button>
        </div>
      </section>

      <section class="card">
        <h2>Оформление</h2>
        <div class="row">
          <button
            classList={{ btn: true, primary: theme() === "dark", ghost: theme() !== "dark" }}
            onClick={() => applyTheme("dark")}
          >
            🌙 Тёмная
          </button>
          <button
            classList={{ btn: true, primary: theme() === "light", ghost: theme() !== "light" }}
            onClick={() => applyTheme("light")}
          >
            ☀ Светлая
          </button>
        </div>
        <p class="muted small">Тема применяется сразу и запоминается.</p>
      </section>
      <section class="card form">
        <h2>Приложение</h2>
        <dl class="kv">
          <dt>Версия</dt>
          <dd>{info()?.version ?? "…"}</dd>
          <dt>Статус движка</dt>
          <dd>{info()?.engine_status ?? "…"}</dd>
          <dt>Порт сайдкара</dt>
          <dd>{info()?.sidecar_port ?? "—"}</dd>
          <dt>Python</dt>
          <dd class="mono">{info()?.python_path ?? "…"}</dd>
          <dt>Файл БД</dt>
          <dd class="mono">{info()?.db_path ?? "…"}</dd>
          <dt>Папка данных</dt>
          <dd class="mono">{info()?.data_dir ?? "…"}</dd>
        </dl>
        <div class="row">
          <button class="btn" onClick={() => api.openDataDir()}>
            Открыть папку данных
          </button>
          <button class="btn" onClick={restart}>
            Перезапустить движок
          </button>
          <button class="btn ghost" onClick={load}>
            Обновить
          </button>
          <button class="btn" onClick={checkAll} disabled={checking()}>
            {checking() ? "Проверяю…" : "Проверить всё"}
          </button>
        </div>

        <Show when={health()}>
          <div classList={{ health: true, ok: health()!.ok, warn: !health()!.ok }}>
            <strong>{health()!.ok ? "✓ Всё отвечает" : "⚠ Есть замечания"}</strong>
            <dl class="kv">
              <dt>Движок</dt>
              <dd>{health()!.engine}</dd>
              <dt>Поиск</dt>
              <dd>{health()!.search}</dd>
            </dl>
          </div>
        </Show>
      </section>

      <section class="card">
        <h2>Данные</h2>
        <p class="muted small">
          Копия базы включает сессии, сообщения, память, навыки, уроки и настройки. Ключи
          провайдеров хранятся отдельно (Credential Manager) и в копию не попадают.
        </p>
        <p class="muted small">
          <b>Ручная копия</b> по умолчанию — обычный файл SQLite: его можно открыть сторонними
          инструментами, перенести на другой компьютер и восстановить. <b>Автокопии</b> в папке
          данных шифруются DPAPI и открываются только под вашей учётной записью; для переноса
          используйте ручную копию.
        </p>
        <div class="row">
          <button class="btn" onClick={() => void backupDb()} disabled={dbBusy()}>
            💾 Сохранить копию базы…
          </button>
          <button class="btn ghost" onClick={() => setRestoreOpen(true)} disabled={dbBusy()}>
            ♻ Восстановить из копии…
          </button>
          <button class="btn ghost" onClick={() => void exportAllData()} disabled={dbBusy()}>
            📦 Экспорт всех чатов (JSON)
          </button>
        </div>
        <label class="inline">
          <input
            type="checkbox"
            checked={encryptBackup()}
            onChange={(event) => setEncryptBackup(event.currentTarget.checked)}
          />
          Шифровать ручную копию (DPAPI)
        </label>
        <p class="muted small">
          Зашифрованную копию нельзя открыть на другом компьютере или под другим пользователем
          Windows — только восстановить в Legamind на этой учётной записи.
        </p>
        <Show when={dbBusy()}>
          <p class="muted small">Работаю с базой…</p>
        </Show>
        <Show when={backupInfo()}>
          <p class="muted small">
            Автокопия создаётся при запуске и далее не чаще раза в 24 часа; хранятся 5 последних.
            Сейчас копий: {backupInfo()!.auto_count}.
            {backupInfo()!.last_auto
              ? ` Последняя: ${new Date(backupInfo()!.last_auto!).toLocaleString("ru-RU")}.`
              : " Пока не создавалась."}
          </p>
          <button class="btn ghost" onClick={() => void api.openPath(backupInfo()!.dir)}>
            📂 Открыть папку копий
          </button>
        </Show>
      </section>

      <section class="card">
        <h2>Векторный RAG</h2>
        <p class="muted small">
          FTS5 работает всегда. Для поиска по смыслу выберите локальную модель эмбеддингов и
          векторизуйте проект через меню «Проект».
        </p>
        <div class="row">
          <select
            value={embeddingModel()}
            onChange={(event) => setEmbeddingModel(event.currentTarget.value)}
          >
            <option value="">Только FTS5</option>
            <For each={embeddingModels()}>
              {(model) => (
                <option value={model.id}>
                  {model.label}
                  {model.kind === "cloud" ? ` · ${model.provider}` : ""}
                </option>
              )}
            </For>
          </select>
          <button class="btn" onClick={() => void saveEmbedding()} disabled={embeddingBusy()}>
            {embeddingBusy() ? "Сохраняю…" : "Сохранить"}
          </button>
        </div>
        <Show when={embeddingModels().length === 0}>
          <p class="muted small">
            Нет моделей эмбеддингов. Добавьте локальную IR-модель в «Моделях» или назначьте
            роль «эмбеддинги» облачной модели в «Провайдерах».
          </p>
        </Show>
        <div class="row">
          <select
            value={rerankerModel()}
            onChange={(event) => setRerankerModel(event.currentTarget.value)}
          >
            <option value="">Без реранкера</option>
            <For
              each={embeddingModels().filter(
                (model) => model.kind === "local" || model.role === "rerank",
              )}
            >
              {(model) => (
                <option value={model.id}>
                  {model.label}
                  {model.kind === "cloud" ? ` · ${model.provider}` : ""}
                </option>
              )}
            </For>
          </select>
          <button class="btn" onClick={() => void saveReranker()} disabled={rerankerBusy()}>
            {rerankerBusy() ? "Сохраняю…" : "Сохранить реранкер"}
          </button>
        </div>
        <p class="muted small">
          Реранкер — локальная OpenVINO-модель; переупорядочивает найденные фрагменты перед
          ответом.
        </p>
      </section>

      <section class="card">
        <h2>Облачное хранение (opt-in)</h2>
        <p class="muted small">
          WebDAV-папка для зашифрованных копий базы. Парольная фраза не сохраняется —
          без неё копию не восстановить. По умолчанию выключено.
        </p>
        <label class="row">
          <input
            type="checkbox"
            checked={cloudEnabled()}
            onChange={(event) => setCloudEnabled(event.currentTarget.checked)}
          />
          <span>Включить облачное хранение</span>
        </label>
        <label class="field">
          <span>URL папки WebDAV</span>
          <input
            value={cloudUrl()}
            placeholder="https://dav.example.com/legamind"
            onInput={(event) => setCloudUrl(event.currentTarget.value)}
          />
        </label>
        <div class="row">
          <label class="field">
            <span>Пользователь</span>
            <input
              value={cloudUser()}
              onInput={(event) => setCloudUser(event.currentTarget.value)}
            />
          </label>
          <label class="field">
            <span>Пароль WebDAV</span>
            <input
              type="password"
              value={cloudPassword()}
              placeholder={cloudHasPassword() ? "сохранён" : ""}
              onInput={(event) => setCloudPassword(event.currentTarget.value)}
            />
          </label>
        </div>
        <label class="field">
          <span>Парольная фраза шифрования (не сохраняется)</span>
          <input
            type="password"
            value={cloudPassphrase()}
            onInput={(event) => setCloudPassphrase(event.currentTarget.value)}
          />
        </label>
        <div class="row">
          <button class="btn" onClick={() => void saveCloud()} disabled={cloudBusy()}>
            {cloudBusy() ? "Сохраняю…" : "Сохранить WebDAV"}
          </button>
          <button class="btn ghost" onClick={() => void uploadCloud()} disabled={cloudBusy()}>
            Загрузить копию
          </button>
          <button class="btn ghost" onClick={() => void restoreCloud()} disabled={cloudBusy()}>
            Восстановить
          </button>
        </div>
      </section>

      <section class="card">
        <h2>Параметры RAG</h2>
        <p class="muted small">
          Расширения и исключаемые папки — через запятую. Лимиты ограничивают размер индекса.
        </p>
        <label class="field">
          <span>Расширения</span>
          <input
            value={ragExtensions()}
            onInput={(event) => setRagExtensions(event.currentTarget.value)}
            placeholder="md, txt, rs, ts"
          />
        </label>
        <label class="field">
          <span>Исключить папки</span>
          <input
            value={ragExcludes()}
            onInput={(event) => setRagExcludes(event.currentTarget.value)}
            placeholder="vendor, dist, generated"
          />
        </label>
        <div class="row">
          <label class="field">
            <span>Макс. файл, КБ</span>
            <input
              type="number"
              min="1"
              max="4096"
              value={ragFileKb()}
              onInput={(event) => setRagFileKb(Number(event.currentTarget.value) || 1)}
            />
          </label>
          <label class="field">
            <span>Макс. файлов</span>
            <input
              type="number"
              min="1"
              max="5000"
              value={ragMaxFiles()}
              onInput={(event) => setRagMaxFiles(Number(event.currentTarget.value) || 1)}
            />
          </label>
          <label class="field">
            <span>Макс. фрагментов</span>
            <input
              type="number"
              min="100"
              max="20000"
              value={ragMaxChunks()}
              onInput={(event) => setRagMaxChunks(Number(event.currentTarget.value) || 100)}
            />
          </label>
        </div>
        <button class="btn" onClick={() => void saveRagConfig()} disabled={ragBusy()}>
          {ragBusy() ? "Сохраняю…" : "Сохранить RAG"}
        </button>
      </section>

      <section class="card">
        <h2>Самопроверка</h2>
        <p class="muted small">
          Проверяет базу, движок, модели, RAG-индекс, провайдеров и ключи, интернет, копии и логи.
        </p>
        <button class="btn" onClick={() => void runSelfCheck()} disabled={checkBusy()}>
          {checkBusy() ? "Проверяю…" : "🔍 Проверить всё"}
        </button>
        <Show when={checks().length > 0}>
          <div class="check-list">
            <For each={checks()}>
              {(check) => (
                <div classList={{ "check-row": true, [check.status]: true }}>
                  <span class="check-icon">
                    {check.status === "ok" ? "✅" : check.status === "warn" ? "⚠" : "❌"}
                  </span>
                  <div>
                    <strong>{check.label}</strong>
                    <p class="muted small">{check.detail}</p>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </section>

      <section class="card">
        <h2>Диагностика</h2>
        <p class="muted small">
          Хвост логов приложения. <code>prompts.log</code> содержит тексты промптов — не
          пересылайте его без необходимости.
        </p>
        <div class="row">
          <label>
            Лог
            <select value={logName()} onChange={(event) => setLogName(event.currentTarget.value)}>
              <option value="app.log">app.log — события</option>
              <option value="audit.log">audit.log — действия с файлами</option>
              <option value="prompts.log">prompts.log — промпты</option>
              <option value="engine_stdout.log">engine_stdout.log</option>
              <option value="engine_stderr.log">engine_stderr.log</option>
            </select>
          </label>
          <label>
            Строк
            <select
              value={String(logLines())}
              onChange={(event) => setLogLines(Number(event.currentTarget.value))}
            >
              <option value="200">200</option>
              <option value="400">400</option>
              <option value="1000">1000</option>
            </select>
          </label>
          <button class="btn" onClick={() => void loadLog()} disabled={logBusy()}>
            {logBusy() ? "Читаю…" : "Показать"}
          </button>
          <button
            class="btn ghost"
            onClick={() =>
              void api.openPath(`${info()?.data_dir ?? ""}\\logs`).catch(() => {})
            }
          >
            📂 Открыть папку логов
          </button>
        </div>
        <Show when={logTail()}>
          <p class="muted small mono">
            {logTail()!.path} · {(logTail()!.size / 1024).toFixed(1)} КБ
          </p>
          <pre class="log-view">{logTail()!.text || "Лог пуст"}</pre>
        </Show>
      </section>
      </Show>

      <Show when={tab() === "privacy"}>
      <section class="card">
        <h2>Приватность</h2>
        <p class="muted small">
          Всё работает локально. Внешние запросы — к провайдерам, которых вы добавили сами, и (если
          включён интернет) к поисковому сервису с текстом вашего запроса. Ключи хранятся в Windows
          Credential Manager (fallback — DPAPI).
        </p>
      </section>
      </Show>

      <ConfirmDialog
        open={resetOpen()}
        title="Сбросить интернет-настройки?"
        message="Провайдер вернётся к «Авто», адреса SearxNG и Google CX очистятся. Сохранённые ключи останутся в Credential Manager."
        confirmLabel="Сбросить"
        onConfirm={() => void resetWeb()}
        onCancel={() => setResetOpen(false)}
      />

      <ConfirmDialog
        open={restoreOpen()}
        title="Восстановить базу из копии?"
        message="Текущие сессии, память и настройки будут заменены содержимым копии. Перед восстановлением автоматически сохраняется резервная копия текущей базы, после — приложение перезапустится."
        confirmLabel="Восстановить"
        danger
        onConfirm={() => void restoreDb()}
        onCancel={() => setRestoreOpen(false)}
      />
    </div>
  );
}
