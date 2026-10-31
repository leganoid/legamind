import { Show, createEffect, createSignal } from "solid-js";

interface Props {
  open: boolean;
  title: string;
  label?: string;
  initial?: string;
  placeholder?: string;
  confirmLabel?: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
}

export default function PromptDialog(props: Props) {
  const [value, setValue] = createSignal(props.initial ?? "");

  createEffect(() => {
    if (props.open) setValue(props.initial ?? "");
  });

  const confirm = () => {
    const text = value().trim();
    if (!text) return;
    props.onConfirm(text);
  };

  return (
    <Show when={props.open}>
      <div class="modal-overlay" onClick={props.onCancel}>
        <div class="modal confirm" onClick={(event) => event.stopPropagation()}>
          <header class="modal-head">
            <strong>{props.title}</strong>
          </header>
          <label class="field">
            <Show when={props.label}>
              <span>{props.label}</span>
            </Show>
            <input
              ref={(element) => setTimeout(() => element.focus(), 0)}
              value={value()}
              placeholder={props.placeholder}
              onInput={(event) => setValue(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  confirm();
                }
                if (event.key === "Escape") props.onCancel();
              }}
            />
          </label>
          <div class="modal-actions">
            <button class="btn primary" onClick={confirm} disabled={!value().trim()}>
              {props.confirmLabel ?? "Сохранить"}
            </button>
            <button class="btn ghost" onClick={props.onCancel}>
              Отмена
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}
