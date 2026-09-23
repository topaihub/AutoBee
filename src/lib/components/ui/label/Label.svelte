<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    for: forId = undefined,
    children,
  }: {
    class?: string;
    for?: string;
    children?: import("svelte").Snippet;
  } = $props();
</script>

<label
  for={forId}
  class={cn("text-xs font-medium text-muted-foreground", className)}
>
  {@render children?.()}
</label>
