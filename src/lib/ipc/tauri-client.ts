/**
 * 唯一允许 `import { invoke }` 的地方（eslint 会拦其他文件）。
 *
 * 独立成模块的理由：Vitest 里要注入假实现，状态层依赖的是 `client.ts`
 * 里的接口，而不是这里 —— 见 ipc/client.ts 的 `PlannerClient`。
 */
import { invoke } from "@tauri-apps/api/core";

/** 1×1 透明 PNG，浏览器预览里瓦片请求兜底用（base64，无 data: 前缀）。 */
const TRANSPARENT_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/**
 * 当前是否运行在 Tauri 桌面窗口里。
 *
 * Tauri v2 的 `invoke` 内部访问 `window.__TAURI_INTERNALS__.invoke`；
 * 在纯浏览器 / dev preview 里这个全局对象是 `undefined`，直接调 `invoke`
 * 会抛 `Cannot read properties of undefined (reading 'invoke')`。
 */
function isTauri(): boolean {
  if (typeof window === "undefined") return false;
  const internals = (window as unknown as { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
  return internals != null;
}

export function invokeCmd<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    // 非 Tauri 环境（浏览器预览 / dev server）：优雅降级，不要崩整个界面。
    if (command === "get_tile") {
      // 瓦片返回透明 PNG，MapCanvas 画上去是空白，但其余 UI 正常渲染。
      return Promise.resolve(TRANSPARENT_PNG as unknown as T);
    }
    return Promise.reject(
      new Error(
        `Tauri IPC 不可用：命令 "${command}" 只能在桌面窗口里调用，当前是浏览器预览环境。`,
      ),
    );
  }
  return invoke<T>(command, args);
}
