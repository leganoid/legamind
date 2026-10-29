import { Show, createSignal, onMount } from "solid-js";
import { api } from "../api";
import type { AppInfo } from "../types";

export default function SettingsView() {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const load = async () => setInfo(await api.appInfo());

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

  onMount(load);

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Настройки</h1>
          <p class="muted small">Только чтение: порт выбирается ОС, путь к БД фиксирован.</p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

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
          Всё работает локально. Единственные внешние запросы — к провайдерам, которых вы добавили сами
          и модель которых выбрали. Ключи хранятся в Windows Credential Manager (fallback — DPAPI).
        </p>
      </section>
    </div>
  );
}
