<script lang="ts">
  /**
   * 单条消息。四种角色各自一个位置与配色，扫一眼就知道谁在说话。
   *
   * `tool` 这一类特别处理：工具返回的 JSON 动辄几千字符，平铺会把对话挤没了，
   * 所以默认只显示一行摘要，点开才给全文。
   */
  import type { ChatItem } from "$lib/state/chat.svelte";
  import { cn } from "$lib/utils/cn";

  let {
    message,
    onretry = undefined,
  }: {
    message: ChatItem;
    /** 失败气泡的重发回调。不传则不显示重发按钮（历史消息没得重发）。 */
    onretry?: (id: string) => void;
  } = $props();

  let expanded = $state(false);

  const mine = $derived(message.role === "user");
  const bodyId = $derived(`tool-body-${message.id}`);
  /** 折叠态给一行预览：去掉换行后截断，否则 JSON 的缩进会把这一行撑开。 */
  const preview = $derived(message.content.replace(/\s+/g, " ").slice(0, 80));
</script>

{#if message.role === "tool"}
  <div class="flex flex-col gap-1 rounded-lg border border-dashed bg-muted/40 px-3 py-2">
    <button
      type="button"
      class="flex items-center gap-2 text-left text-[11px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
      aria-expanded={expanded}
      aria-controls={bodyId}
      onclick={() => (expanded = !expanded)}
    >
      <svg
        width="10"
        height="10"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="3"
        stroke-linecap="round"
        stroke-linejoin="round"
        class={cn("shrink-0 transition-transform", expanded && "rotate-90")}
        aria-hidden="true"
      >
        <path d="m9 18 6-6-6-6" />
      </svg>
      <span class="font-medium">{message.toolName ?? "工具"}</span>
      <span class="min-w-0 truncate text-muted-foreground">{expanded ? "收起" : preview}</span>
    </button>
    {#if expanded}
      <pre
        id={bodyId}
        class="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-md bg-card p-2 font-mono text-[11px] text-muted-foreground">{message.content}</pre>
    {/if}
  </div>
{:else if message.role === "system"}
  <p class="mx-auto max-w-[80%] text-center text-[11px] text-muted-foreground">{message.content}</p>
{:else}
  <div class={cn("flex w-full", mine ? "justify-end" : "justify-start")}>
    <div class={cn("flex max-w-[85%] flex-col gap-1", mine && "items-end")}>
      <div
        class={cn(
          "whitespace-pre-wrap break-words rounded-lg px-3 py-2 text-sm",
          mine ? "bg-primary text-primary-foreground" : "border bg-card text-foreground",
          message.failed && "border border-destructive",
        )}
      >{message.content}</div>
      {#if message.failed}
        <!-- 失败要看得见：没有标记的话用户会以为消息发出去了，然后重打一遍。 -->
        <p class="flex items-center gap-2 text-[11px] text-destructive">
          <span aria-hidden="true">⚠</span>
          <span>发送失败</span>
          {#if onretry}
            <button
              type="button"
              class="underline underline-offset-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              onclick={() => onretry?.(message.id)}>重新发送</button
            >
          {/if}
        </p>
      {:else if message.model}
        <span class="text-[11px] text-muted-foreground">{message.model}</span>
      {/if}
    </div>
  </div>
{/if}
