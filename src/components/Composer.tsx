import { createSignal } from "solid-js";

interface Props {
  busy: boolean;
  disabled: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
}

/** Ввод: Enter — отправить, Shift+Enter — перенос, Ctrl+Enter — тоже отправить. */
export default function Composer(props: Props) {
  const [text, setText] = createSignal("");
  let inputRef: HTMLTextAreaElement | undefined;

  const submit = () => {
    const value = text().trim();
    if (!value || props.busy || props.disabled) return;
    props.onSend(value);
    setText("");
    if (inputRef) inputRef.style.height = "auto";
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submit();
    }
  };

  const autoGrow = (event: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
    const el = event.currentTarget;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 220) + "px";
  };

  return (
    <div class="composer">
      <textarea
        ref={inputRef}
        value={text()}
        placeholder="Сообщение… (Enter — отправить, Shift+Enter — перенос)"
        rows={2}
        disabled={props.disabled}
        onInput={(e) => {
          setText(e.currentTarget.value);
          autoGrow(e);
        }}
        onKeyDown={onKeyDown}
      />
      <div class="composer-actions">
        {props.busy ? (
          <button class="btn danger" onClick={() => props.onStop()}>
            Стоп
          </button>
        ) : (
          <button class="btn primary" disabled={props.disabled || !text().trim()} onClick={submit}>
            Отправить
          </button>
        )}
      </div>
    </div>
  );
}
