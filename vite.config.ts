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
      // Игнорируем служебные каталоги сборки и временные файлы редакторов:
      // watcher падает с EBUSY на заблокированных .tmp внутри workspace.
      ignored: ["**/src-tauri/**", "**/engine/**", "**/*.tmpdir/**", "**/*.tmp"],
    },
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
