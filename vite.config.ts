import { defineConfig } from "vitest/config";
import { resolve } from "node:path";

// Three pages: the popup (index.html), the setup wizard, and settings.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    assetsInlineLimit: 8192,
    rollupOptions: {
      input: {
        popup: resolve(__dirname, "index.html"),
        wizard: resolve(__dirname, "wizard.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.ts"],
  },
});
