/// <reference types="vitest" />
import { defineConfig } from "vite";
import { resolve } from "path";

export default defineConfig({
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:43117",
        changeOrigin: true,
        configure(proxy) {
          proxy.on("proxyReq", (proxyReq) => {
            proxyReq.setHeader("Origin", "http://127.0.0.1:43117");
            proxyReq.setHeader("Referer", "http://127.0.0.1:43117/");
          });
        },
      },
    },
  },
  build: {
    manifest: true,
    cssCodeSplit: true,
    modulePreload: false,
    target: "esnext",
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, "index.html"),
        landing: resolve(import.meta.dirname, "landing.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      thresholds: {
        statements: 75,
        branches: 70,
        functions: 72,
        lines: 80,
      },
    },
  },
});
