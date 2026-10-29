import { For, Show, createEffect, createMemo } from "solid-js";
import type { ModelInfo } from "../types";

interface Props {
  models: ModelInfo[];
  providerLabels: Record<string, string>;
  value: string;
  onChange: (id: string) => void;
  disabled?: boolean;
  placeholder?: string;
}

/** Выбор модели: локальные и облачные сгруппированы по провайдерам. */
export default function ModelPicker(props: Props) {
  let selectRef: HTMLSelectElement | undefined;

  const local = createMemo(() => props.models.filter((m) => m.kind === "local"));
  const groups = createMemo(() => {
    const map = new Map<string, ModelInfo[]>();
    for (const model of props.models.filter((m) => m.kind === "cloud")) {
      const list = map.get(model.provider) ?? [];
      list.push(model);
      map.set(model.provider, list);
    }
    return [...map.entries()];
  });

  // Список моделей приходит асинхронно: значение нужно выставить после отрисовки опций.
  createEffect(() => {
    props.models;
    const value = props.value;
    if (selectRef && value) {
      selectRef.value = value;
    }
  });

  return (
    <select
      ref={selectRef}
      class="model-picker"
      value={props.value}
      disabled={props.disabled}
      onChange={(event) => props.onChange(event.currentTarget.value)}
    >
      <option value="">{props.placeholder ?? "Выберите модель"}</option>
      <Show when={local().length > 0}>
        <optgroup label="Локальные">
          <For each={local()}>
            {(model) => (
              <option value={model.id}>
                {model.label} · {model.device}
                {model.loaded ? " · загружена" : ""}
              </option>
            )}
          </For>
        </optgroup>
      </Show>
      <For each={groups()}>
        {([provider, list]) => (
          <optgroup label={props.providerLabels[provider] ?? provider}>
            <For each={list}>{(model) => <option value={model.id}>{model.label}</option>}</For>
          </optgroup>
        )}
      </For>
    </select>
  );
}
