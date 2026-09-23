<script lang="ts" module>
  /**
   * 分段选择（地铁/公交/步行这类互斥切换）。
   * 不做动画 —— hallmark：即时反馈优先于过渡效果。
   */
  import { cn } from "$lib/utils/cn";
</script>

<script lang="ts">
  let {
    options,
    value = $bindable<string>(),
    onchange = () => {},
    class: className = "",
  }: {
    options: Array<{ value: string; label: string }>;
    value: string;
    onchange?: (v: string) => void;
    class?: string;
  } = $props();
</script>

<div
  role="tablist"
  class={cn("inline-flex rounded-md bg-muted p-0.5", className)}
>
  {#each options as opt (opt.value)}
    <button
      type="button"
      role="tab"
      aria-selected={value === opt.value}
      class={cn(
        "rounded-[5px] px-3 h-8 text-sm font-medium whitespace-nowrap",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        value === opt.value
          ? "bg-card text-foreground shadow-sm"
          : "text-muted-foreground hover:text-foreground",
      )}
      onclick={() => {
        value = opt.value;
        onchange(opt.value);
      }}
    >
      {opt.label}
    </button>
  {/each}
</div>
