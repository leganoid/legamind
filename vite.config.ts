import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// Tauri ждёт фиксированный порт dev-сервера (см. tauri.conf.json -> devUrl).
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      // Служебные каталоги сборки и временные файлы редакторов не смотрим:
      // watcher падает с EBUSY на заблокированных .tmp внутри workspace.
      ignored: (path: string) =>
        path.includes("node_modules") ||
        path.includes("src-tauri") ||
        path.includes("/engine/") ||
        path.includes("\\engine\\") ||
        path.includes(".tmpdir") ||
        path.endsWith(".tmp"),
    },
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
