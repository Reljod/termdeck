import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri drives the dev server, so the port is fixed and failures must be loud
// rather than silently landing on a different port the app will not open.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "safari15",
    sourcemap: false,
  },
});
