import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The Tauri desktop shell serves this build in production and points at the Vite
// dev server (port 1420) in development.
export default defineConfig(({ mode }) => ({
  // Layout fixtures need the QA entrypoint, but it must never enter the desktop bundle.
  define: mode === "layout" ? { "import.meta.env.DEV": "true" } : {},
  plugins: [
    svelte({
      onwarn(warning, handler) {
        if (warning.code === "css_unused_selector") {
          throw new Error(`Unused Svelte CSS selector: ${warning.message}`);
        }
        handler(warning);
      },
    }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: mode === "layout" ? "test-results/layout-app" : "dist",
    // Match a reasonable modern WebView baseline.
    target: "es2021",
  },
  preview: { host: "127.0.0.1", port: 1420, strictPort: true },
}));
