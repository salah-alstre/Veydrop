/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Built straight into the desktop app's resources; the Rust server serves it.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@shared": fileURLToPath(new URL("../shared", import.meta.url)) } },
  server: {
    port: 1421,
    strictPort: true,
    fs: { allow: [".."] },
    // Develop against a running desktop engine: VEYDROP_ENGINE=http://192.168.x.x:47821
    proxy: process.env.VEYDROP_ENGINE
      ? { "/api": { target: process.env.VEYDROP_ENGINE, changeOrigin: false, headers: { Origin: process.env.VEYDROP_ENGINE } } }
      : undefined,
  },
  build: {
    target: ["es2020", "safari15", "chrome100"],
    outDir: "../desktop/src-tauri/resources/mobile",
    emptyOutDir: true,
    sourcemap: false,
    assetsInlineLimit: 0,
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test-setup.ts"],
  },
});
