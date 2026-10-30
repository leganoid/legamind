import { Show } from "solid-js";

interface Props {
  open: boolean;
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog(props: Props) {
  return (
    <Show when={props.open}>
      <div class="modal-overlay" onClick={props.onCancel}>
        <div class="modal confirm" onClick={(event) => event.stopPropagation()}>
          <header class="modal-head">
            <strong>{props.title}</strong>
          </header>
          <Show when={props.message}>
            <p class="muted small confirm-text">{props.message}</p>
          </Show>
          <div class="modal-actions">
            <button
              classList={{ btn: true, primary: !props.danger, danger: props.danger }}
              onClick={props.onConfirm}
            >
              {props.confirmLabel ?? "Подтвердить"}
            </button>
            <button class="btn ghost" onClick={props.onCancel}>
              {props.cancelLabel ?? "Отмена"}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}
