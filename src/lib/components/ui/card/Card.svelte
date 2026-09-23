<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    children,
  }: {
    class?: string;
    children?: import("svelte").Snippet;
  } = $props();
</script>

<div
  class={cn("rounded-lg border bg-card text-card-foreground shadow-sm", className)}
>
  {@render children?.()}
</div>
