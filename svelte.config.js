// Tauri 没有 Node 服务端做 SSR，所以用 adapter-static + fallback 走 SPA 模式。
// 见 https://svelte.dev/docs/kit/single-page-apps 与 https://v2.tauri.app/start/frontend/sveltekit/
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
