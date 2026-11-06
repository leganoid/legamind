export const FONT_FAMILIES: { label: string; value: string }[] = [
  { label: "Системный", value: "" },
  { label: "Segoe UI", value: '"Segoe UI", system-ui, sans-serif' },
  { label: "Arial", value: "Arial, sans-serif" },
  { label: "Georgia", value: "Georgia, serif" },
  { label: "Consolas", value: "Consolas, monospace" },
  { label: "Comic Sans MS", value: '"Comic Sans MS", cursive' },
];

export const FONT_SIZES = [12, 13, 14, 15, 16, 17, 18, 20];

const KEY_FAMILY = "legamind.font.family";
const KEY_SIZE = "legamind.font.size";
const KEY_TEXT = "legamind.font.text";
const KEY_ACCENT = "legamind.font.accent";

export function applyAppearance() {
  const root = document.documentElement;
  const family = localStorage.getItem(KEY_FAMILY) ?? "";
  root.style.fontFamily = family;
  const size = Number(localStorage.getItem(KEY_SIZE) ?? "14");
  root.style.fontSize = `${Number.isFinite(size) && size >= 10 ? size : 14}px`;
  const text = localStorage.getItem(KEY_TEXT) ?? "";
  if (text) root.style.setProperty("--text", text);
  else root.style.removeProperty("--text");
  const accent = localStorage.getItem(KEY_ACCENT) ?? "";
  if (accent) root.style.setProperty("--accent", accent);
  else root.style.removeProperty("--accent");
}

export function saveAppearance(patch: {
  family?: string;
  size?: number;
  text?: string;
  accent?: string;
}) {
  if (patch.family !== undefined) localStorage.setItem(KEY_FAMILY, patch.family);
  if (patch.size !== undefined) localStorage.setItem(KEY_SIZE, String(patch.size));
  if (patch.text !== undefined) localStorage.setItem(KEY_TEXT, patch.text);
  if (patch.accent !== undefined) localStorage.setItem(KEY_ACCENT, patch.accent);
  applyAppearance();
}

export function resetAppearance() {
  localStorage.removeItem(KEY_FAMILY);
  localStorage.removeItem(KEY_SIZE);
  localStorage.removeItem(KEY_TEXT);
  localStorage.removeItem(KEY_ACCENT);
  applyAppearance();
}

export function loadAppearance() {
  return {
    family: localStorage.getItem(KEY_FAMILY) ?? "",
    size: Number(localStorage.getItem(KEY_SIZE) ?? "14"),
    text: localStorage.getItem(KEY_TEXT) ?? "",
    accent: localStorage.getItem(KEY_ACCENT) ?? "",
  };
}
