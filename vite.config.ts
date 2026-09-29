import { defineConfig } from "vite";
import { resolve } from "node:path";

// Two pages: the floating panel and the settings window. Tauri serves the
// dev server in `tauri dev` and the built `dist` in a release.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    // WebView2 is Chromium, which has light-dark() from 123. For anything
    // older the build rewrites it into variables settled where a colour is
    // declared rather than where it is used, and a theme inside another —
    // the theme thumbnails, a rail kept dark — takes the page's instead.
    cssTarget: "chrome123",
    outDir: "dist",
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, "index.html"),
        settings: resolve(import.meta.dirname, "settings.html"),
      },
    },
  },
});
