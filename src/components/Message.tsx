import { Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import type { ChatMessageRow } from "../types";
import { renderMarkdown } from "../markdown";

interface Props {
  message: ChatMessageRow;
  modelLabel: (id: string | null) => string;
  streaming?: boolean;
  actions?: JSX.Element;
}

export default function Message(props: Props) {
  const isUser = () => props.message.role === "user";
  const [copied, setCopied] = createSignal(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.message.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер обмена может быть недоступен — молча игнорируем.
    }
  };

  return (
    <article classList={{ message: true, user: isUser(), assistant: !isUser() }}>
      <header>
        <span class="message-author">{isUser() ? "Вы" : props.modelLabel(props.message.model_id)}</span>
        <Show when={!isUser() && props.message.tokens}>
          <span class="message-meta">{props.message.tokens} ток.</span>
        </Show>
        <Show when={props.streaming}>
          <span class="message-meta streaming">печатает…</span>
        </Show>
        <span class="message-tools">
          <button class="icon-btn" title="Скопировать сообщение" onClick={copy}>
            {copied() ? "✓" : "⧉"}
          </button>
          {props.actions}
        </span>
      </header>
      <Show
        when={!isUser()}
        fallback={<div class="message-body">{props.message.content}</div>}
      >
        <div class="message-body markdown" innerHTML={renderMarkdown(props.message.content)} />
      </Show>
    </article>
  );
}
