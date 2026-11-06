import { For, Show, createMemo, createSignal } from "solid-js";
import { api } from "../api";
import type { ModelInfo } from "../types";

interface Props {
  models: ModelInfo[];
}

export default function ImagesView(props: Props) {
  const imageModels = createMemo(() => props.models.filter((model) => model.role === "image"));
  const [model, setModel] = createSignal("");
  const activeModel = createMemo(() => model() || imageModels()[0]?.id || "");
  const [prompt, setPrompt] = createSignal("");
  const [negative, setNegative] = createSignal("");
  const [size, setSize] = createSignal("512x512");
  const [steps, setSteps] = createSignal(20);
  const [guidance, setGuidance] = createSignal(7.5);
  const [seed, setSeed] = createSignal(0);
  const [image, setImage] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const generate = async () => {
    if (!prompt().trim()) return;
    if (!activeModel()) {
      setError("Нет локальной image-модели. Добавьте её в «Моделях».");
      return;
    }
    const [width, height] = size().split("x").map((value) => Number(value));
    setBusy(true);
    setError(null);
    try {
      setImage(
        await api.imageGenerate({
          model: activeModel(),
          prompt: prompt().trim(),
          negative: negative().trim(),
          width,
          height,
          steps: steps(),
          seed: seed(),
          guidanceScale: guidance(),
        }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Изображения</h1>
          <p class="muted small">
            Локальная генерация через OpenVINO Text2Image. Модель должна лежать в C:\Models.
          </p>
        </div>
      </header>

      <Show when={imageModels().length > 0} fallback={<p class="muted">Локальных image-моделей нет.</p>}>
        <section class="card">
          <div class="row">
            <label class="field">
              <span>Модель</span>
              <select value={activeModel()} onChange={(event) => setModel(event.currentTarget.value)}>
                <For each={imageModels()}>
                  {(model) => <option value={model.id}>{model.label}</option>}
                </For>
              </select>
            </label>
            <label class="field">
              <span>Размер</span>
              <select value={size()} onChange={(event) => setSize(event.currentTarget.value)}>
                <option value="512x512">512×512</option>
                <option value="768x768">768×768</option>
                <option value="512x768">512×768</option>
              </select>
            </label>
            <label class="field">
              <span>Шаги</span>
              <input
                type="number"
                min="1"
                max="40"
                value={steps()}
                onInput={(event) => setSteps(Number(event.currentTarget.value) || 20)}
              />
            </label>
            <label class="field">
              <span>Guidance</span>
              <input
                type="number"
                min="1"
                max="20"
                step="0.5"
                value={guidance()}
                onInput={(event) => setGuidance(Number(event.currentTarget.value) || 7.5)}
              />
            </label>
            <label class="field">
              <span>Seed</span>
              <input
                type="number"
                value={seed()}
                onInput={(event) => setSeed(Number(event.currentTarget.value) || 0)}
              />
            </label>
          </div>
          <label class="field">
            <span>Промпт</span>
            <textarea
              class="image-prompt"
              rows={4}
              value={prompt()}
              onInput={(event) => setPrompt(event.currentTarget.value)}
              placeholder="a cat sitting on a windowsill, soft light, detailed fur, 50mm photo"
            />
          </label>
          <label class="field">
            <span>Негативный промпт</span>
            <textarea
              class="image-negative"
              rows={2}
              value={negative()}
              onInput={(event) => setNegative(event.currentTarget.value)}
              placeholder="blurry, low quality, deformed, extra limbs, text, watermark"
            />
          </label>
          <p class="hint">
            Для SD1.5: 512×512 и 20–30 шагов. 768×768 без дообучения часто даёт артефакты и
            повторяющиеся дома. Guidance 7–8. Промпт и негативный промпт лучше писать
            по-английски — CLIP у SD1.5 плохо понимает русский.
          </p>
          <button class="btn primary" onClick={() => void generate()} disabled={busy() || !prompt().trim()}>
            {busy() ? "Генерирую…" : "Сгенерировать"}
          </button>
        </section>
      </Show>

      <Show when={error()}>
        <div class="banner error">{error()}</div>
      </Show>

      <Show when={image()}>
        <section class="card">
          <img class="generated-image" src={image()!} alt="Сгенерированное изображение" />
          <div class="row">
            <a class="btn ghost" href={image()!} download="legamind-image.png">
              Скачать PNG
            </a>
          </div>
        </section>
      </Show>
    </div>
  );
}
