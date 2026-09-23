<script lang="ts">
  /**
   * 轻量模态：背板点击关闭、Esc 关闭、焦点落在内容里。
   * 腾讯 GL 地图会建立 WebGL 合成层，弹窗必须使用最大安全 z-index 才能接收输入。
   */
  import { cn } from "$lib/utils/cn";

  let {
    open = $bindable(false),
    title = "",
    class: className = "",
    children,
  }: {
    open?: boolean;
    title?: string;
    class?: string;
    children?: import("svelte").Snippet;
  } = $props();

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div
    class="fixed inset-0 z-[2147483646] bg-foreground/40"
    aria-hidden="true"
    onclick={() => (open = false)}
  ></div>
  <div
    role="dialog"
    aria-modal="true"
    aria-label={title}
    class={cn(
      "fixed left-1/2 top-1/2 z-[2147483647] isolate w-[min(640px,calc(100vw-2rem))] max-h-[85vh] overflow-y-auto",
      "-translate-x-1/2 -translate-y-1/2 rounded-lg border bg-card shadow-lg pointer-events-auto",
      className,
    )}
  >
    <header class="flex items-center justify-between border-b px-4 py-3">
      <h2 class="text-sm font-semibold">{title}</h2>
      <button
        type="button"
        aria-label="关闭"
        class="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onclick={() => (open = false)}
      >
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M2 2l10 10M12 2L2 12" />
        </svg>
      </button>
    </header>
    <div class="p-4">
      {@render children?.()}
    </div>
  </div>
{/if}
