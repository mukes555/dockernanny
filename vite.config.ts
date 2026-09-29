import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import { readFileSync } from "node:fs";
const host = process.env.TAURI_DEV_HOST;

// src-tauri/Cargo.toml holds the one version number; Tauri, the updater and
// the release tag all follow it, and so does the window.
const cargoToml: string = readFileSync(new URL("./src-tauri/Cargo.toml", import.meta.url), "utf8");
const version = /^version = "([^"]+)"/m.exec(cargoToml)?.[1] ?? "dev";

// 1430 rather than Tauri's usual 1420, so it can run beside other Tauri projects.
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
