import { For, Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import type { ChatMessageRow, SourceRow } from "../types";
import { renderMarkdown } from "../markdown";

interface Props {
  message: ChatMessageRow;
  modelLabel: (id: string | null) => string;
  streaming?: boolean;
  actions?: JSX.Element;
  sources?: SourceRow[];
}

export default function Message(props: Props) {
  const isUser = () => props.message.role === "user";
  const [copied, setCopied] = createSignal(false);
  const [copiedUrl, setCopiedUrl] = createSignal<string | null>(null);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.message.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер обмена может быть недоступен — молча игнорируем.
    }
  };

  const copyLink = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url);
      setCopiedUrl(url);
      window.setTimeout(() => setCopiedUrl(null), 1500);
    } catch {
      // ignore
    }
  };

  const domain = (url: string) => {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return url;
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
      <Show when={props.sources && props.sources.length > 0}>
        <details class="sources">
          <summary class="muted small">Источники ({props.sources!.length})</summary>
          <ol>
            <For each={props.sources}>
              {(source) => (
                <li>
                  <a
                    href={source.url}
                    title="Скопировать ссылку"
                    onClick={(event) => {
                      event.preventDefault();
                      void copyLink(source.url);
                    }}
                  >
                    {source.title?.trim() || domain(source.url)}
                  </a>
                  <span class="muted small"> — {domain(source.url)}</span>
                  <Show when={copiedUrl() === source.url}>
                    <span class="muted small"> · скопировано</span>
                  </Show>
                </li>
              )}
            </For>
          </ol>
        </details>
      </Show>
    </article>
  );
}
