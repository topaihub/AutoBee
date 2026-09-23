<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    value = $bindable(""),
    options,
    disabled = false,
    id = undefined,
    onchange = () => {},
  }: {
    class?: string;
    value?: string;
    options: Array<{ value: string; label: string; disabled?: boolean }>;
    disabled?: boolean;
    id?: string;
    onchange?: (e: Event & { currentTarget: HTMLSelectElement }) => void;
  } = $props();
</script>

<select
  bind:value
  {disabled}
  {id}
  class={cn(
    "flex h-9 w-full appearance-none rounded-md border border-input bg-card px-3 text-sm",
    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
    "disabled:cursor-not-allowed disabled:opacity-50",
    className,
  )}
  {onchange}
>
  {#each options as opt (opt.value)}
    <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
  {/each}
</select>
