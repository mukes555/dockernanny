import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// 1430 rather than Tauri's usual 1420, so it can run beside other Tauri projects.
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  // pnpm sets npm_package_version for every script, so the top bar shows the version from package.json.
  define: { __APP_VERSION__: JSON.stringify(process.env.npm_package_version ?? "dev") },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
