import { Show } from "solid-js";
import type { ChatMessageRow } from "../types";

interface Props {
  message: ChatMessageRow;
  modelLabel: (id: string | null) => string;
  streaming?: boolean;
}

export default function Message(props: Props) {
  const isUser = () => props.message.role === "user";
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
      </header>
      <div class="message-body">{props.message.content}</div>
    </article>
  );
}
