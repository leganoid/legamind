import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  CandidateCheck,
  DatasetProgress,
  DatasetRow,
  ExtractedItem,
  FinetuneStatus,
  LessonRow,
  ModelInfo,
  Session,
} from "../types";
import ModelPicker from "./ModelPicker";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
}

export default function LearningView(props: Props) {
  const [lessons, setLessons] = createSignal<LessonRow[]>([]);
  const [lessonText, setLessonText] = createSignal("");
  const [datasets, setDatasets] = createSignal<DatasetRow[]>([]);
  const [sessions, setSessions] = createSignal<Session[]>([]);
  const [status, setStatus] = createSignal<FinetuneStatus | null>(null);
  const [teacher, setTeacher] = createSignal("");
  const [datasetName, setDatasetName] = createSignal("");
  const [sourceMode, setSourceMode] = createSignal<"session" | "text">("text");
  const [sessionId, setSessionId] = createSignal("");
  const [sourceText, setSourceText] = createSignal("");
  const [count, setCount] = createSignal(10);
  const [progress, setProgress] = createSignal<DatasetProgress | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [copied, setCopied] = createSignal(false);
  const [candidates, setCandidates] = createSignal<ExtractedItem[]>([]);
  const [checks, setChecks] = createSignal<Record<string, CandidateCheck>>({});
  const [extractSession, setExtractSession] = createSignal("");
  const [extracting, setExtracting] = createSignal(false);
  const [targetDataset, setTargetDataset] = createSignal("");
  const [newDatasetName, setNewDatasetName] = createSignal("");

  const loadLessons = async () => setLessons(await api.lessonsList());
  const loadDatasets = async () => setDatasets(await api.datasetsList());
  const loadStatus = async () => setStatus(await api.finetuneStatus());

  const addLesson = async () => {
    const text = lessonText().trim();
    if (!text) return;
    try {
      await api.lessonAdd(text, null);
      setLessonText("");
      setError(null);
      await loadLessons();
    } catch (err) {
      setError(String(err));
    }
  };

  const toggleLesson = async (lesson: LessonRow) => {
    await api.lessonSetEnabled(lesson.id, !lesson.enabled);
    await loadLessons();
  };

  const removeLesson = async (lesson: LessonRow) => {
    await api.lessonDelete(lesson.id);
    await loadLessons();
  };

  const generate = async () => {
    if (!teacher()) {
      setError("Выберите модель-учителя");
      return;
    }
    const name = datasetName().trim() || `dataset-${new Date().toISOString().slice(0, 10)}`;
    setBusy(true);
    setError(null);
    setNotice(null);
    setProgress(null);
    try {
      const row = await api.datasetGenerate({
        name,
        teacher_model: teacher(),
        source_text: sourceMode() === "text" ? sourceText() : null,
        session_id: sourceMode() === "session" ? sessionId() : null,
        count: count(),
      });
      setNotice(`Генерация запущена: ${row.name}. Прогресс — ниже.`);
      await loadDatasets();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const removeDataset = async (row: DatasetRow) => {
    if (!window.confirm(`Удалить датасет «${row.name}» и его файл?`)) return;
    await api.datasetDelete(row.id);
    await loadDatasets();
  };

  // По умолчанию берём самую свежую сессию — обычно именно в ней готовый результат.
  createEffect(() => {
    if (!extractSession() && sessions().length > 0) {
      setExtractSession(sessions()[0].id);
    }
  });

  const lastDataset = createMemo(() => datasets()[0] ?? null);

  const extractFromSession = async () => {
    if (!extractSession()) {
      setError("Выберите сессию");
      return;
    }
    if (!teacher()) {
      setError("Выберите модель-учителя");
      return;
    }
    setExtracting(true);
    setError(null);
    setNotice(null);
    setCandidates([]);
    setChecks({});
    try {
      const items = await api.sessionExtract(extractSession(), teacher());
      setCandidates(items);
      if (items.length === 0) {
        setNotice("Итогов не найдено — попробуйте другую модель-учителя");
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setExtracting(false);
    }
  };

  const checkCandidate = async (item: ExtractedItem) => {
    try {
      const result = await api.candidateCheck(teacher(), item.instruction, item.answer);
      setChecks((prev) => ({ ...prev, [item.id]: result }));
    } catch (err) {
      setError(String(err));
    }
  };

  const acceptCandidate = async (item: ExtractedItem) => {
    try {
      let datasetId = targetDataset();
      if (!datasetId) {
        const name = newDatasetName().trim() || `from-session-${new Date().toISOString().slice(0, 10)}`;
        const row = await api.datasetCreate(name);
        datasetId = row.id;
        setTargetDataset(row.id);
      }
      const result = await api.datasetAppend(datasetId, [
        { instruction: item.instruction, answer: item.answer },
      ]);
      setNotice(`Добавлено: ${result.added}, пропущено дублей: ${result.skipped}`);
      setCandidates((prev) => prev.filter((c) => c.id !== item.id));
      await loadDatasets();
    } catch (err) {
      setError(String(err));
    }
  };

  const rejectCandidate = (item: ExtractedItem) => {
    setCandidates((prev) => prev.filter((c) => c.id !== item.id));
  };

  const verdictLabel = (verdict: string) =>
    verdict === "already"
      ? "модель уже умеет"
      : verdict === "partial"
        ? "частично знает"
        : "новое";

  // Учителем по умолчанию — облачная модель (быстрее и качественнее для датасетов),
  // иначе первая доступная локальная.
  createEffect(() => {
    if (!teacher() && props.models.length > 0) {
      const cloud = props.models.find((m) => m.kind === "cloud");
      setTeacher(cloud?.id ?? props.models[0].id);
    }
  });

  const commands = createMemo(() => {
    const dataset = lastDataset();
    const datasetPath = dataset?.path ?? "%APPDATA%\\com.legamind.app\\datasets\\<file>.jsonl";
    const base = status()?.hf_models[0] ?? "Qwen/Qwen2.5-1.5B-Instruct";
    const name = dataset?.name ?? "my-lora";
    return [
      "pip install torch transformers peft datasets optimum-intel nncf",
      `python scripts/finetune_qlora.py --dataset "${datasetPath}" --base "${base}" --output adapters/${name} --epochs 2`,
      `python scripts/export_openvino.py --base "${base}" --adapter adapters/${name} --output C:\\Models\\${name}-int4-ov`,
    ].join("\n");
  });

  const copyCommands = async () => {
    try {
      await navigator.clipboard.writeText(commands());
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  onMount(async () => {
    await Promise.all([loadLessons(), loadDatasets(), loadStatus()]);
    setSessions(await api.sessionsList());
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await listen<DatasetProgress>("dataset:progress", async (event) => {
        const payload = event.payload;
        setProgress(payload);
        if (payload.phase === "done") {
          setNotice(`Датасет готов: ${payload.examples} примеров`);
          setBusy(false);
          await loadDatasets();
        }
        if (payload.phase === "error") {
          setError(payload.error ?? "Ошибка генерации датасета");
          setBusy(false);
        }
      }),
    );
    onCleanup(() => unlisteners.forEach((fn) => fn()));
  });

  const percent = createMemo(() => {
    const p = progress();
    if (!p || p.total === 0) return 0;
    return Math.min(100, Math.round((p.done / p.total) * 100));
  });

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Обучение</h1>
          <p class="muted small">
            Самообучение без переобучения (правила и память) и подготовка дообучения весов:
            датасет генерирует облачная модель-учитель, обучение — скриптами LoRA.
          </p>
        </div>
      </header>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>
      <Show when={notice()}>
        <div class="banner info">{notice()}</div>
      </Show>

      <section class="card form">
        <h2>Самообучение: правила</h2>
        <p class="muted small">
          Оценка 👎 в чате с комментарием превращается в правило. Правила подмешиваются в
          system-промпт (до 1500 символов) и действуют сразу — веса не меняются.
        </p>
        <div class="row">
          <input
            class="grow"
            value={lessonText()}
            placeholder="Например: не извиняйся, начинай сразу с ответа"
            onInput={(e) => setLessonText(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void addLesson();
            }}
          />
          <button class="btn primary" onClick={addLesson} disabled={!lessonText().trim()}>
            Добавить правило
          </button>
        </div>

        <Show when={lessons().length === 0}>
          <p class="empty-inline">Правил пока нет.</p>
        </Show>
        <div class="lesson-list">
          <For each={lessons()}>
            {(lesson) => (
              <div classList={{ "lesson-item": true, off: !lesson.enabled }}>
                <label class="checkbox-label">
                  <input
                    type="checkbox"
                    checked={lesson.enabled}
                    onChange={() => toggleLesson(lesson)}
                  />
                </label>
                <span class="lesson-text">{lesson.text}</span>
                <button class="icon-btn danger" title="Удалить" onClick={() => removeLesson(lesson)}>
                  ✕
                </button>
              </div>
            )}
          </For>
        </div>
      </section>

      <section class="card form">
        <h2>Дообучение: датасет дистилляции</h2>
        <p class="muted small">
          Учитель (облачная или локальная модель) генерирует пары «инструкция → ответ» по вашему
          материалу. Файл в формате JSONL подходит для LoRA-обучения.
        </p>

        <div class="form-grid">
          <label>
            Название
            <input
              value={datasetName()}
              onInput={(e) => setDatasetName(e.currentTarget.value)}
              placeholder="например: dumps-format"
            />
          </label>
          <label>
            Учитель
            <ModelPicker
              models={props.models}
              providerLabels={props.providerLabels}
              value={teacher()}
              onChange={setTeacher}
              placeholder="Выберите модель"
            />
          </label>
          <label>
            Примеров
            <input
              type="number"
              min={5}
              max={200}
              value={count()}
              onInput={(e) => setCount(Number(e.currentTarget.value) || 30)}
            />
          </label>
        </div>

        <div class="row">
          <label class="inline">
            <input
              type="radio"
              checked={sourceMode() === "text"}
              onChange={() => setSourceMode("text")}
            />
            Свой текст
          </label>
          <label class="inline">
            <input
              type="radio"
              checked={sourceMode() === "session"}
              onChange={() => setSourceMode("session")}
            />
            Из сессии
          </label>
        </div>

        <Show when={sourceMode() === "text"}>
          <textarea
            rows={4}
            placeholder="Вставьте конспект, документацию, переписку — всё, на чём модель должна учиться"
            value={sourceText()}
            onInput={(e) => setSourceText(e.currentTarget.value)}
          />
        </Show>
        <Show when={sourceMode() === "session"}>
          <select value={sessionId()} onChange={(e) => setSessionId(e.currentTarget.value)}>
            <option value="">Выберите сессию…</option>
            <For each={sessions()}>{(s) => <option value={s.id}>{s.title}</option>}</For>
          </select>
        </Show>

        <div class="form-actions">
          <button class="btn primary" onClick={generate} disabled={busy()}>
            {busy() ? "Генерирую…" : "Сгенерировать датасет"}
          </button>
          <Show when={progress()}>
            <div class="progress-wrap">
              <div class="progress-bar">
                <div class="progress-fill" style={{ width: `${percent()}%` }} />
              </div>
              <span class="muted small">
                {progress()!.phase === "done"
                  ? `готово: ${progress()!.examples} примеров`
                  : `${progress()!.examples} из ${progress()!.total}`}
              </span>
            </div>
          </Show>
        </div>

        <Show when={datasets().length > 0}>
          <div class="dataset-list">
            <For each={datasets()}>
              {(row) => (
                <div class="dataset-item">
                  <div class="grow">
                    <strong>{row.name}</strong>
                    <span class="muted small">
                      {" "}
                      · {row.examples} примеров · {row.teacher_model ?? "—"}
                    </span>
                    <div class="mono small muted">{row.path}</div>
                  </div>
                  <button class="btn small danger" onClick={() => removeDataset(row)}>
                    Удалить
                  </button>
                </div>
              )}
            </For>
          </div>
        </Show>
      </section>

      <section class="card form">
        <h2>Из сессии: итоговый результат</h2>
        <p class="muted small">
          Модель вытащит из готового диалога только финальные решения (код, команды, шаги), без
          промежуточной отладки. Кандидаты проверяются на дубли с уже собранными датасетами;
          кнопка «Проверить знание» просит модель ответить самой и сравнивает с эталоном.
        </p>

        <div class="form-grid">
          <label>
            Сессия
            <select value={extractSession()} onChange={(e) => setExtractSession(e.currentTarget.value)}>
              <option value="">Выберите сессию…</option>
              <For each={sessions()}>{(s) => <option value={s.id}>{s.title}</option>}</For>
            </select>
          </label>
          <label>
            Куда добавлять
            <select value={targetDataset()} onChange={(e) => setTargetDataset(e.currentTarget.value)}>
              <option value="">— новый датасет —</option>
              <For each={datasets()}>
                {(row) => <option value={row.id}>{row.name} ({row.examples})</option>}
              </For>
            </select>
          </label>
          <Show when={!targetDataset()}>
            <label>
              Название нового
              <input
                value={newDatasetName()}
                onInput={(e) => setNewDatasetName(e.currentTarget.value)}
                placeholder="из-сессии-..."
              />
            </label>
          </Show>
        </div>

        <div class="form-actions">
          <button class="btn primary" onClick={extractFromSession} disabled={extracting()}>
            {extracting() ? "Извлекаю итоги…" : "Извлечь итоги"}
          </button>
        </div>

        <Show when={candidates().length > 0}>
          <div class="candidate-list">
            <For each={candidates()}>
              {(item) => {
                const check = () => checks()[item.id];
                return (
                  <div class="candidate-card">
                    <div class="row space-between">
                      <strong class="candidate-instruction">{item.instruction}</strong>
                      <div class="row">
                        <Show when={item.duplicate_of}>
                          <span class="badge off" title={item.duplicate_of ?? ""}>
                            дубль {Math.round(item.duplicate_score * 100)}%
                          </span>
                        </Show>
                        <Show when={check()}>
                          <span
                            classList={{
                              badge: true,
                              on: check()!.verdict === "new",
                              off: check()!.verdict !== "new",
                            }}
                          >
                            {verdictLabel(check()!.verdict)} · {Math.round(check()!.coverage * 100)}%
                          </span>
                        </Show>
                        <For each={item.warnings}>
                          {(warning) => <span class="badge off">{warning}</span>}
                        </For>
                      </div>
                    </div>
                    <details class="candidate-answer">
                      <summary class="muted small">Показать ответ ({item.answer.length} симв.)</summary>
                      <pre>{item.answer}</pre>
                    </details>
                    <Show when={check()}>
                      <div class="muted small candidate-model-answer">
                        Ответ модели: {check()!.model_answer.slice(0, 220)}
                        {check()!.model_answer.length > 220 ? "…" : ""}
                      </div>
                    </Show>
                    <div class="row">
                      <button class="btn small" onClick={() => checkCandidate(item)}>
                        Проверить знание
                      </button>
                      <button class="btn small primary" onClick={() => acceptCandidate(item)}>
                        Принять в датасет
                      </button>
                      <button class="btn small ghost" onClick={() => rejectCandidate(item)}>
                        Отклонить
                      </button>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>
        </Show>
      </section>

      <section class="card form">
        <h2>Дообучение весов (LoRA → OpenVINO)</h2>
        <p class="muted small">
          В приложении обучение не запускается: нужны PyTorch, исходные HF-веса базовой модели и
          часы GPU-времени. Ниже — что уже установлено и готовые команды. Готовую папку
          `*-int4-ov` можно добавить в реестр моделей.
        </p>

        <Show when={status()}>
          <div class="finetune-status">
            <div class="row">
              <span classList={{ badge: true, on: status()!.ready, off: !status()!.ready }}>
                {status()!.ready ? "окружение готово" : "не готово"}
              </span>
              <span class="muted small">Python: {status()!.python}</span>
              <span class="muted small">Устройство: {status()!.device}</span>
              <button class="btn small ghost" onClick={loadStatus}>
                Проверить снова
              </button>
            </div>
            <div class="pkg-grid">
              <For each={Object.entries(status()!.packages)}>
                {([name, present]) => (
                  <span classList={{ "pkg": true, ok: present, bad: !present }}>
                    {present ? "✅" : "❌"} {name}
                  </span>
                )}
              </For>
            </div>
            <Show when={!status()!.ready}>
              <p class="muted small">{status()!.note}</p>
            </Show>
            <Show when={status()!.hf_models.length > 0}>
              <p class="muted small">HF-веса с safetensors: {status()!.hf_models.join(", ")}</p>
            </Show>
            <Show when={status()!.hf_models.length === 0}>
              <p class="muted small">
                HF-весов (safetensors) в {status()!.model_root} нет — для дообучения нужны исходные
                веса базовой модели, IR-папки не подходят.
              </p>
            </Show>
          </div>
        </Show>

        <div class="code-block">
          <div class="row space-between">
            <span class="muted small">Команды</span>
            <button class="btn small" onClick={copyCommands}>
              {copied() ? "Скопировано" : "Скопировать"}
            </button>
          </div>
          <pre>{commands()}</pre>
        </div>
        <p class="muted small">
          После экспорта положите папку в `C:\Models` и добавьте её как модель, чтобы Legamind
          увидел дообученный вариант.
        </p>
      </section>
    </div>
  );
}
