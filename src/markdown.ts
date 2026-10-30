// Рендер markdown-ответов модели: подсветка кода + санитайзер.
import { Marked } from "marked";
import { markedHighlight } from "marked-highlight";
import hljs from "highlight.js/lib/common";
import DOMPurify from "dompurify";

const marked = new Marked(
  markedHighlight({
    emptyLangClass: "hljs",
    langPrefix: "hljs language-",
    highlight(code, lang) {
      const language = lang && hljs.getLanguage(lang) ? lang : "plaintext";
      try {
        return hljs.highlight(code, { language }).value;
      } catch {
        return code;
      }
    },
  }),
  { gfm: true, breaks: true },
);

/** Сворачивает блок рассуждений `<think>…</think>` в раскрывающийся details. */
function collapseThink(text: string): string {
  return text.replace(/<think>([\s\S]*?)<\/think>/gi, (_match, inner: string) => {
    const escaped = inner.replace(/&/g, "&amp;").replace(/</g, "&lt;");
    return `\n\n<details class="think-block"><summary>Рассуждения модели</summary><div class="think-text">${escaped}</div></details>\n\n`;
  });
}

/** Возвращает безопасный HTML для вставки через innerHTML. */
export function renderMarkdown(text: string): string {
  const html = marked.parse(collapseThink(text), { async: false }) as string;
  const clean = DOMPurify.sanitize(html, {
    ADD_ATTR: ["target", "rel"],
  });
  // Внешние ссылки — в новой вкладке и без доступа к opener.
  return clean.replace(/<a /g, '<a target="_blank" rel="noreferrer noopener" ');
}

/** Одна строка без разметки — для заголовков сессий и превью. */
export function plainText(text: string): string {
  return text.replace(/[#*_`>\[\]]/g, "").replace(/\s+/g, " ").trim();
}
