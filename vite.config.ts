import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望一个固定端口，且失败时不要自动切换
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // 安卓 WebView 目标
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
