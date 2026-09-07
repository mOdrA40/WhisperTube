import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", "**/skills/**"]
    }
  },
  envPrefix: ["VITE_", "TAURI_"],
  test: {
    environment: "jsdom",
  },
});
