import { For, Show, createSignal } from "solid-js";

interface Step {
  title: string;
  text: string;
}

const STEPS: Step[] = [
  {
    title: "Сессии слева",
    text: "Создавайте чаты, переключайтесь между ними, закрепляйте важные кнопкой 📌. Поиск ищет и по тексту сообщений.",
  },
  {
    title: "Модель и её роль",
    text: "Облачную модель подключите ключом в «Провайдерах». Локальные OpenVINO-модели в установщик не входят: скачайте IR-папку и добавьте её в «Моделях». В чате видны только чат-модели.",
  },
  {
    title: "Композер",
    text: "Enter — отправить, Shift+Enter — перенос. 📎 прикладывает текст, PDF и изображения, ✨ Пресеты вставляют готовую инструкцию, /help — список команд.",
  },
  {
    title: "Проект и RAG",
    text: "Папка проекта задаётся чипом «Проект». По ней строится индекс с учётом .gitignore; изменения подхватываются автоматически. Файловые действия требуют подтверждения.",
  },
  {
    title: "Совет и настройки",
    text: "«Совет» задаёт один вопрос нескольким моделям и может собрать сводку арбитра. Провайдеры, роли моделей, тема, диагностика и резервные копии — в «Настройках». Ctrl+K открывает быстрый переход.",
  },
];

export default function Tour(props: { onClose: () => void }) {
  const [index, setIndex] = createSignal(0);
  const last = () => index() >= STEPS.length - 1;

  const finish = () => {
    props.onClose();
  };

  const openModels = () => {
    props.onClose();
    window.dispatchEvent(new CustomEvent("legamind:navigate", { detail: "models" }));
  };

  return (
    <div class="modal-overlay" onClick={finish}>
      <div class="tour-card" onClick={(event) => event.stopPropagation()}>
        <div class="tour-progress">
          <For each={STEPS}>
            {(_, position) => (
              <span classList={{ "tour-dot": true, active: position() === index() }} />
            )}
          </For>
        </div>
        <h2>{STEPS[index()].title}</h2>
        <p>{STEPS[index()].text}</p>
        <div class="row space-between">
          <button class="btn ghost" onClick={finish}>
            Пропустить
          </button>
          <div class="row">
            <button
              class="btn ghost"
              disabled={index() === 0}
              onClick={() => setIndex((value) => Math.max(0, value - 1))}
            >
              Назад
            </button>
            <Show when={last()}>
              <button class="btn ghost" onClick={openModels}>
                Настроить модели
              </button>
            </Show>
            <button
              class="btn primary"
              onClick={() => {
                if (last()) {
                  finish();
                } else {
                  setIndex((value) => value + 1);
                }
              }}
            >
              {last() ? "Начать" : "Далее"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
