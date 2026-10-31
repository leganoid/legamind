import { For, Show, createSignal } from "solid-js";
import type { JSX } from "solid-js";
import type { ChatMessageRow, SourceRow, ToolActivity } from "../types";
import { renderMarkdown } from "../markdown";

interface Props {
  message: ChatMessageRow;
  modelLabel: (id: string | null) => string;
  streaming?: boolean;
  actions?: JSX.Element;
  sources?: SourceRow[];
  tools?: ToolActivity[];
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
    <article
      id={`msg-${props.message.id}`}
      classList={{ message: true, user: isUser(), assistant: !isUser() }}
    >
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
      <Show when={props.tools && props.tools.length > 0}>
        <details class="tool-cards">
          <summary class="muted small">Инструменты ({props.tools!.length})</summary>
          <For each={props.tools}>
            {(item) => (
              <div
                classList={{
                  "tool-card": true,
                  error: item.status === "done" && !item.ok,
                }}
              >
                <span class="tool-state">
                  {item.status === "pending" ? "⏳" : item.status === "running" ? "…" : item.ok ? "✓" : "✗"}
                </span>
                <span class="tool-name">{item.tool}</span>
                <Show when={item.path}>
                  <span class="muted small mono">{item.path}</span>
                </Show>
                <Show when={item.summary}>
                  <span class="muted small">{item.summary}</span>
                </Show>
                <Show when={item.status === "pending"}>
                  <span class="muted small">ждёт подтверждения</span>
                </Show>
                <Show when={item.detail}>
                  <details class="tool-detail">
                    <summary class="muted small">
                      {item.tool === "run_command" ? "команда и вывод" : "подробности"}
                    </summary>
                    <pre>{item.detail}</pre>
                  </details>
                </Show>
              </div>
            )}
          </For>
        </details>
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
