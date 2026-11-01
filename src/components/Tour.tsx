import { For, createSignal } from "solid-js";

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
    text: "Модель выбирается вверху. В чате видны только чат-модели; эмбеддинги, реранкеры и генераторы изображений живут на странице «Модели» и в ролях провайдера.",
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
    title: "Настройки",
    text: "Провайдеры, роли моделей, тема, диагностика и резервные копии — в «Настройках». Ctrl+K открывает быстрый переход.",
  },
];

export default function Tour(props: { onClose: () => void }) {
  const [index, setIndex] = createSignal(0);
  const last = () => index() >= STEPS.length - 1;

  return (
    <div class="modal-overlay" onClick={() => props.onClose()}>
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
          <button class="btn ghost" onClick={() => props.onClose()}>
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
            <button
              class="btn primary"
              onClick={() => {
                if (last()) {
                  props.onClose();
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
