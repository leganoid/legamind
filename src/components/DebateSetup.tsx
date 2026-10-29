import { For, Show, createSignal } from "solid-js";
import type { ModelInfo } from "../types";

export interface DebateLaunch {
  task: string;
  participants: string[];
  rounds: number;
  judge: string | null;
  max_tokens: number;
}

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  busy: boolean;
  onStart: (launch: DebateLaunch) => void;
  onStop: () => void;
}

export default function DebateSetup(props: Props) {
  const [task, setTask] = createSignal("");
  const [participants, setParticipants] = createSignal<string[]>([]);
  const [rounds, setRounds] = createSignal(2);
  const [judge, setJudge] = createSignal<string>("synthesis");
  const [maxTokens, setMaxTokens] = createSignal(768);
  const [error, setError] = createSignal<string | null>(null);

  const toggle = (id: string) => {
    const current = participants();
    if (current.includes(id)) {
      setParticipants(current.filter((p) => p !== id));
      return;
    }
    if (current.length >= 4) {
      setError("Не больше 4 участников");
      return;
    }
    setError(null);
    setParticipants([...current, id]);
  };

  const start = () => {
    if (task().trim().length < 5) {
      setError("Опишите задачу подробнее");
      return;
    }
    if (participants().length < 2) {
      setError("Выберите минимум 2 участников");
      return;
    }
    setError(null);
    props.onStart({
      task: task().trim(),
      participants: participants(),
      rounds: rounds(),
      judge: judge() === "synthesis" ? null : judge(),
      max_tokens: maxTokens(),
    });
  };

  return (
    <section class="card form debate-setup">
      <h2>Новые дебаты</h2>
      <label class="stack">
        Задача
        <textarea
          rows={3}
          value={task()}
          placeholder="Например: спроектировать формат хранения дампов прошивки"
          onInput={(e) => setTask(e.currentTarget.value)}
          disabled={props.busy}
        />
      </label>

      <div class="stack">
        Участники ({participants().length}/4)
        <div class="picker-grid">
          <For each={props.models}>
            {(model) => (
              <label classList={{ "pick-item": true, selected: participants().includes(model.id) }}>
                <input
                  type="checkbox"
                  checked={participants().includes(model.id)}
                  disabled={props.busy}
                  onChange={() => toggle(model.id)}
                />
                <span>{model.label}</span>
                <span class="muted small">
                  {model.kind === "local" ? model.device : props.providerLabels[model.provider] ?? model.provider}
                </span>
              </label>
            )}
          </For>
        </div>
      </div>

      <div class="form-grid narrow">
        <label>
          Раундов
          <select value={rounds()} onChange={(e) => setRounds(Number(e.currentTarget.value))} disabled={props.busy}>
            <option value={1}>1</option>
            <option value={2}>2</option>
            <option value={3}>3</option>
          </select>
        </label>
        <label>
          Судья
          <select value={judge()} onChange={(e) => setJudge(e.currentTarget.value)} disabled={props.busy}>
            <option value="synthesis">Синтез всех (голосование)</option>
            <For each={props.models}>{(model) => <option value={model.id}>{model.label}</option>}</For>
          </select>
        </label>
        <label>
          Токенов на реплику
          <input
            type="number"
            min={256}
            max={2048}
            step={128}
            value={maxTokens()}
            onInput={(e) => setMaxTokens(Number(e.currentTarget.value) || 768)}
            disabled={props.busy}
          />
        </label>
      </div>

      <div class="form-actions">
        <Show
          when={props.busy}
          fallback={
            <button class="btn primary" onClick={start}>
              Начать дебаты
            </button>
          }
        >
          <button class="btn danger" onClick={() => props.onStop()}>
            Остановить
          </button>
        </Show>
        <Show when={error()}>
          <span class="check-result bad">{error()}</span>
        </Show>
      </div>

      <Show when={props.models.length === 0}>
        <p class="muted small">
          Моделей нет. Добавьте провайдера на экране «Провайдеры» или запустите локальный движок.
        </p>
      </Show>
    </section>
  );
}
