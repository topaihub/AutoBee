<script lang="ts">
  import "../app.css";
  import type { Snippet } from "svelte";
  import { createDefaultNotify, createI18n, KitNotifier, setKitContext } from "@ouraihub/aether-ui";
  import { createLogger } from "@ouraihub/aether-ui/logs";
  import { createTauriPorts } from "@ouraihub/aether-ui/tauri";
  import { assistant } from "$lib/state/assistant.svelte";

  let { children }: { children: Snippet } = $props();

  // Aether kit：问一问、大模型 / MCP / Skills 设置与日志。
  const ports = createTauriPorts({ providers: true, chat: true, chatHistory: true, skills: true, mcpClient: true });
  // 路线 / 生活圈工具的结果要画到地图上，在 ChatPanel 拿到 port 之前包一层。
  if (ports.chat) assistant.wrap(ports.chat);
  const i18n = createI18n("zh-CN");
  const notify = createDefaultNotify();
  setKitContext({ ports, i18n, notify, logger: createLogger(ports.logs) });
</script>

{@render children()}

<KitNotifier {notify} {i18n} />
