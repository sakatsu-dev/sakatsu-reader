import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The Tauri desktop shell serves this build in production and points at the Vite
// dev server (port 1420) in development.
export default defineConfig({
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
    // Match a reasonable modern WebView baseline.
    target: "es2021",
  },
});
