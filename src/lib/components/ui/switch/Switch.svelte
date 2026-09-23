<script lang="ts">
  /** 开关。原生 checkbox 换皮，键盘可达性原样保留。 */
  import { cn } from "$lib/utils/cn";

  let {
    checked = $bindable(false),
    disabled = false,
    label = "",
    onchange = () => {},
  }: {
    checked?: boolean;
    disabled?: boolean;
    label?: string;
    onchange?: (v: boolean) => void;
  } = $props();
</script>

<label class={cn("inline-flex items-center gap-2", disabled && "opacity-50")}>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label || "switch"}
    {disabled}
    class={cn(
      "relative h-5 w-9 rounded-full border transition-colors",
      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
      checked ? "bg-primary border-primary" : "bg-input border-input",
    )}
    onclick={() => {
      checked = !checked;
      onchange(checked);
    }}
  >
    <span
      class={cn(
        "absolute top-0.5 size-3.5 rounded-full bg-card shadow-sm transition-[left]",
        checked ? "left-[18px]" : "left-0.5",
      )}
    ></span>
  </button>
  {#if label}
    <span class="text-sm">{label}</span>
  {/if}
</label>
