// Tauri WebView 里的 SPA:没有服务端。Aether 的 KitContext 在 layout 初始化时就要
// `window`/Tauri IPC,dev 下 SSR 会直接 500(window is not defined)。
export const ssr = false;
