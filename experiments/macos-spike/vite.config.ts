import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1422,
    strictPort: true,
    fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
    watch: { ignored: ["**/src-tauri/**", "**/crates/**/target/**", "**/tests/artifacts/**"] },
  },
  build: { target: "safari15", outDir: "dist", emptyOutDir: true },
});
