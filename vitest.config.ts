import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// svelte 插件必须在场：state 层是 .svelte.ts，里面用了 $state 这类 rune，
// 不编译的话 vitest 直接报语法错。domain 层的测试用相对路径互相引用，
// 不依赖 $lib 别名，几十毫秒就能起。
export default defineConfig({
  plugins: [svelte()],
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
