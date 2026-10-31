import { For, Show, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { AppInfo, BackupInfo, LogTail, WebConfigView, WebTestResult } from "../types";
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
