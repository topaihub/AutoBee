import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [tailwindcss(), sveltekit()],

  // 1. 不让 Vite 清屏盖掉 Rust 的报错
  clearScreen: false,
  // 2. Tauri 要固定端口，占用了就直接失败而不是换一个
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // 3. src-tauri 由 cargo 自己看着，Vite 不要监听
      ignored: ["**/src-tauri/**"],
    },
  },
}));
