import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("/node_modules/three/")) return "three";
          if (id.includes("/node_modules/skinview3d/") || id.includes("/node_modules/skinview-utils/")) return "skinview3d";
        },
      },
    },
  },
  envPrefix: ["VITE_"],
  test: { environment: "node" },
});
