import { For, Show, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { AppInfo, WebConfigView, WebTestResult } from "../types";

export default function SettingsView() {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const [web, setWeb] = createSignal<WebConfigView | null>(null);
  const [provider, setProvider] = createSignal("ddg");
  const [searxngUrl, setSearxngUrl] = createSignal("");
  const [tavilyKey, setTavilyKey] = createSignal("");
  const [braveKey, setBraveKey] = createSignal("");
  const [webTest, setWebTest] = createSignal<WebTestResult | null>(null);
  const [testing, setTesting] = createSignal(false);

  const load = async () => setInfo(await api.appInfo());

  const loadWeb = async () => {
    const config = await api.webConfigGet();
    setWeb(config);
    setProvider(config.provider);
    setSearxngUrl(config.searxng_url);
  };

  const saveWeb = async (patch: { enabled?: boolean }) => {
    setError(null);
    try {
      const config = await api.webConfigSet({
        enabled: patch.enabled,
        provider: provider(),
        searxng_url: searxngUrl(),
        tavily_key: tavilyKey().trim() || undefined,
        brave_key: braveKey().trim() || undefined,
      });
      setWeb(config);
      setTavilyKey("");
      setBraveKey("");
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

  const restart = async () => {
    setError(null);
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
  });

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Настройки</h1>
          <p class="muted small">Порт выбирается ОС, путь к БД фиксирован. Здесь же — доступ в интернет.</p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

      <section class="card form">
        <h2>Интернет для моделей</h2>
        <p class="muted small">
          Если включено, перед ответом Legamind ищет запрос в интернете (DuckDuckGo без ключа,
          SearxNG, Tavily или Brave) и вставляет результаты в контекст. Ссылки-источники
          сохраняются под ответом.
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
                <option value="auto">Авто: ключ → SearxNG → Wikipedia → SO → GitHub → Yahoo</option>
                <option value="wikipedia">Wikipedia (без ключа)</option>
                <option value="stackexchange">Stack Exchange (без ключа)</option>
                <option value="github">GitHub (без ключа)</option>
                <option value="searxng">SearxNG (свой инстанс)</option>
                <option value="tavily">Tavily (API-ключ)</option>
                <option value="brave">Brave Search (API-ключ)</option>
                <option value="yahoo">Yahoo (последний резерв)</option>
                <option value="bing">Bing (best effort)</option>
                <option value="ddg">DuckDuckGo (best effort)</option>
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
          </div>

          <div class="row">
            <button class="btn primary" onClick={() => saveWeb({})}>
              Сохранить
            </button>
            <button class="btn" onClick={testSearch} disabled={testing()}>
              {testing() ? "Проверяю…" : "Проверить поиск"}
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
        </div>
      </section>

      <section class="card">
        <h2>Приватность</h2>
        <p class="muted small">
          Всё работает локально. Внешние запросы — к провайдерам, которых вы добавили сами, и (если
          включён интернет) к поисковому сервису с текстом вашего запроса. Ключи хранятся в Windows
          Credential Manager (fallback — DPAPI).
        </p>
      </section>
    </div>
  );
}
