import { defineConfig } from "vite";
import { resolve } from "node:path";

// Two pages: the floating panel and the settings window. Tauri serves the
// dev server in `tauri dev` and the built `dist` in a release.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, "index.html"),
        settings: resolve(import.meta.dirname, "settings.html"),
      },
    },
  },
});
