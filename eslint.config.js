import js from "@eslint/js";
import globals from "globals";
import svelte from "eslint-plugin-svelte";
import tseslint from "typescript-eslint";
import svelteConfig from "./svelte.config.js";

/**
 * Tailwind 硬编码色阶的禁令（与 ai-installer 同一条规则）。
 *
 * 组件里只准用语义 token（bg-card / text-muted-foreground / border），
 * 深浅两套值在 app.css 里定义一次。否则浅色模式要靠 !important 逐条覆盖，
 * 那正是 scrcpy-gui 里 130 行覆盖样式的来源。
 */
const TAILWIND_COLOR_SCALES = String.raw`(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)`;
const COLOR_PREFIXES = String.raw`(?:bg|text|border|ring|outline|divide|from|via|to|shadow|decoration|accent|caret|fill|stroke|placeholder)`;

const HARDCODED_COLOR_CLASS = new RegExp(
  String.raw`\b${COLOR_PREFIXES}-${TAILWIND_COLOR_SCALES}-\d{2,3}\b`,
);

const ARBITRARY_COLOR_CLASS = new RegExp(
  String.raw`\b${COLOR_PREFIXES}-\[(?:#|rgba?\(|hsla?\(|okl(?:ch|ab)\(|la[bch]\(|white|black|transparent\b)`,
  "i",
);

const INLINE_STYLE_COLOR = new RegExp(
  String.raw`(?:^|[;{"'\s])(?:background|background-color|color|border(?:-\w+)?-color|outline-color|fill|stroke|caret-color|accent-color)\s*:\s*(?!var\(|currentColor|inherit|transparent|none|unset|initial)[a-z#(]`,
  "i",
);

const noHardcodedColorScale = {
  meta: {
    type: "problem",
    docs: { description: "禁止在组件里写死颜色，只用 app.css 的语义 token" },
    schema: [],
    messages: {
      scale:
        "禁止硬编码色阶 `{{ cls }}`：请改用语义 token（bg-card / text-muted-foreground / border 等）。深浅两套值在 app.css 里定义。",
      arbitrary:
        "禁止任意值颜色 `{{ cls }}`：请改用语义 token，或在 app.css 里加一个 token 再引用。`bg-[var(--your-token)]` 是允许的。",
      inline:
        "禁止在行内 style 里写死颜色（`{{ cls }}`）：类名检查看不到这里，但效果一样是写死配色。",
    },
  },
  create(context) {
    const CHECKS = [
      { pattern: HARDCODED_COLOR_CLASS, messageId: "scale" },
      { pattern: ARBITRARY_COLOR_CLASS, messageId: "arbitrary" },
      { pattern: INLINE_STYLE_COLOR, messageId: "inline" },
    ];

    function check(node, raw) {
      for (const { pattern, messageId } of CHECKS) {
        const match = pattern.exec(raw);
        if (match) {
          context.report({ node, messageId, data: { cls: match[0].trim() } });
          return;
        }
      }
    }
    return {
      Literal(node) {
        if (typeof node.value === "string") check(node, node.value);
      },
      TemplateElement(node) {
        if (node.value?.raw) check(node, node.value.raw);
      },
      SvelteLiteral(node) {
        if (typeof node.value === "string") check(node, node.value);
      },
      SvelteText(node) {
        if (typeof node.value === "string") check(node, node.value);
      },
    };
  },
};

export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs.recommended,
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
      parserOptions: {
        extraFileExtensions: [".svelte"],
        svelteConfig,
      },
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
        extraFileExtensions: [".svelte"],
        svelteConfig,
      },
    },
  },
  {
    // 色阶禁令只管我们自己写的代码。ui/ 将来由 shadcn-svelte CLI 生成，
    // 是上游产物，升级时可能带进别的写法，不要因此卡住 lint。
    files: ["src/**/*.{ts,svelte}"],
    ignores: ["src/lib/components/ui/**"],
    plugins: { local: { rules: { "no-hardcoded-color-scale": noHardcodedColorScale } } },
    rules: { "local/no-hardcoded-color-scale": "error" },
  },
  {
    // Tauri 调用统一经 ipc/tauri-client.ts，其余地方不准直接 invoke。
    files: ["src/**/*.{ts,svelte}"],
    ignores: ["src/lib/ipc/tauri-client.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/api/core",
              importNames: ["invoke"],
              message: "Tauri 调用必须经由 src/lib/ipc/tauri-client.ts。",
            },
          ],
        },
      ],
    },
  },
  {
    // domain 层是纯函数层，必须能在 Vitest 里脱离 Tauri 与 Svelte 单测。
    files: ["src/lib/domain/**"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            { group: ["@tauri-apps/*"], message: "domain 层不得依赖 Tauri。" },
            { group: ["svelte", "svelte/*", "$app/*"], message: "domain 层不得依赖 Svelte。" },
            { group: ["$lib/components/*"], message: "domain 层不得依赖视图组件。" },
          ],
        },
      ],
    },
  },
  {
    // 业务组件不准发起 IPC：只接收 props 与回调，调用由 route/state 负责。
    // 拦的是会发请求的模块；types.ts 是纯类型，组件必须能引它标 props。
    files: ["src/lib/components/app/**"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["$lib/ipc/tauri-client", "$lib/ipc/client"],
              message: "业务组件只接收 props 和回调，IPC 调用由 route/store 负责。",
            },
          ],
        },
      ],
    },
  },
  {
    ignores: ["build/", "dist/", ".svelte-kit/", "src-tauri/", "node_modules/", "tools/", ".omp/"],
  },
);
